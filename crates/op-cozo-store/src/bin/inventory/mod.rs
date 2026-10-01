//! Bounded inventory of explicitly supplied prepared bytes, not deployed state.
use anyhow::{Context, Result, bail, ensure};
use op_cozo_store::{
    GraphAssertion, GraphEdge, GraphEvidence, GraphNode, GraphProvenance, GraphProvenanceKind,
    GraphSnapshot,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path, PathBuf},
};

const MAX_ENTRIES: usize = 45_000;
const MAX_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_TEXT: u64 = 1024 * 1024;
const MAX_JSON: usize = 64 * 1024 * 1024;
const GRAPH_DIR: &str = "usr/share/op-dbus/graphs";

pub fn run(args: Vec<String>) -> Result<()> {
    let mut flags = BTreeMap::new();
    ensure!(
        args.len().is_multiple_of(2),
        "usage: system_graph_inventory (--prepared DIR | --source-graph FILE) --output FILE --namespace NAME --generated-at RFC3339"
    );
    for pair in args.chunks(2) {
        ensure!(
            [
                "--prepared",
                "--source-graph",
                "--output",
                "--namespace",
                "--generated-at"
            ]
            .contains(&pair[0].as_str()),
            "unknown option {}",
            pair[0]
        );
        ensure!(
            flags.insert(pair[0].clone(), pair[1].clone()).is_none(),
            "duplicate option {}",
            pair[0]
        );
    }
    let required = |key: &str| flags.get(key).context(format!("required option {key}"));
    let namespace = required("--namespace")?;
    let generated_at = required("--generated-at")?;
    chrono::DateTime::parse_from_rfc3339(generated_at).context("invalid --generated-at")?;
    let output = PathBuf::from(required("--output")?);
    let mut snapshot = match (flags.get("--prepared"), flags.get("--source-graph")) {
        (Some(root), None) => {
            ensure!(
                namespace.starts_with("release:"),
                "prepared namespace must start with release:"
            );
            prepared(Path::new(root), namespace, generated_at, &output)?
        }
        (None, Some(input)) => {
            ensure!(
                namespace.starts_with("source:"),
                "source namespace must start with source:"
            );
            source_graph(Path::new(input), namespace, generated_at)?
        }
        _ => bail!("supply exactly one of --prepared or --source-graph"),
    };
    snapshot.edges = merge_edges(snapshot.edges)?;
    op_cozo_store::system_graph_snapshot_digest(&snapshot)?;
    atomic_write(&output, &serde_json::to_vec_pretty(&snapshot)?)?;
    println!(
        "{}: {} nodes, {} edges; prepared source only",
        namespace,
        snapshot.nodes.len(),
        snapshot.edges.len()
    );
    Ok(())
}

fn attrs(value: Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .expect("attributes object")
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
fn evidence(
    source: &str,
    locator: impl Into<String>,
    assertion: GraphAssertion,
) -> Vec<GraphEvidence> {
    vec![GraphEvidence {
        source: source.into(),
        locator: locator.into(),
        assertion,
    }]
}
fn edge(
    src: impl Into<String>,
    relation: &str,
    dst: impl Into<String>,
    source: &str,
    locator: impl Into<String>,
    attributes: Value,
) -> GraphEdge {
    GraphEdge {
        src: src.into(),
        relation: relation.into(),
        dst: dst.into(),
        evidence: evidence(source, locator, GraphAssertion::Declared),
        attributes: attrs(attributes),
    }
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn safe_rel(path: &Path) -> Result<String> {
    ensure!(
        path.components().all(|c| matches!(c, Component::Normal(_))),
        "non-relative path"
    );
    let s = path.to_str().context("non-UTF8 prepared path")?;
    ensure!(
        s.len() <= 450 && !s.chars().any(char::is_control),
        "unsupported prepared path"
    );
    Ok(s.into())
}
fn sensitive(path: &str) -> bool {
    path.split('/')
        .any(|s| matches!(s, "secrets" | "credentials" | "private"))
        || path.ends_with(".key")
        || path.ends_with(".env")
        || path.ends_with("/environment")
}
fn category(path: &str) -> &'static str {
    if sensitive(path) {
        "protected_configuration"
    } else if path.starts_with("sv/") {
        "service_definition"
    } else if path.contains("container-artifacts/") {
        "container_artifact"
    } else if path.contains("plugin-blobs/") {
        "sealed_catalog"
    } else if path.contains("dashboard/") {
        "ui_asset"
    } else if path.contains("manifests/") || path == "MANIFEST" {
        "provenance"
    } else if path.starts_with("etc/")
        || path.ends_with(".toml")
        || path.ends_with(".conf")
        || path.ends_with(".hocon")
    {
        "configuration"
    } else {
        "artifact"
    }
}

struct Builder {
    nodes: BTreeMap<String, GraphNode>,
    edges: Vec<GraphEdge>,
    paths: BTreeSet<String>,
    bytes: u64,
}
impl Builder {
    fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: Vec::new(),
            paths: BTreeSet::new(),
            bytes: 0,
        }
    }
    fn node(&mut self, id: String, kind: &str, label: String, attributes: Value) -> Result<()> {
        ensure!(
            self.nodes.len() < MAX_ENTRIES,
            "prepared graph entry bound exceeded"
        );
        self.nodes.insert(
            id.clone(),
            GraphNode {
                id,
                kind: kind.into(),
                label,
                attributes: attrs(attributes),
            },
        );
        Ok(())
    }
    fn walk(&mut self, root: &Path, relative: &Path, depth: usize) -> Result<()> {
        ensure!(depth <= 64, "prepared directory depth bound exceeded");
        let dir = root.join(relative);
        ensure!(
            fs::symlink_metadata(&dir)?.is_dir(),
            "prepared directory replaced during inventory"
        );
        let mut entries = Vec::new();
        for entry in fs::read_dir(&dir)? {
            ensure!(
                entries.len() + self.nodes.len() < MAX_ENTRIES,
                "prepared directory entry bound exceeded"
            );
            entries.push(entry?);
        }
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let rel = relative.join(entry.file_name());
            let name = safe_rel(&rel)?;
            if name == GRAPH_DIR || name.starts_with(&format!("{GRAPH_DIR}/")) {
                continue;
            }
            let path = root.join(&rel);
            let metadata = fs::symlink_metadata(&path)?;
            let id = format!("file:{name}");
            let file_type = if metadata.is_dir() {
                "directory"
            } else if metadata.is_file() {
                "regular"
            } else if metadata.file_type().is_symlink() {
                "symlink"
            } else {
                bail!("special file not permitted in prepared tree: {name}")
            };
            let mut properties = attrs(
                json!({"path":name,"file_type":file_type,"size":metadata.len(),"mode":metadata.mode() & 0o7777,"category":category(&name),"authority":"prepared_release"}),
            );
            if name == "MANIFEST" {
                properties.remove("size");
            }
            if metadata.is_file() && name != "MANIFEST" && !sensitive(&name) {
                self.bytes = self
                    .bytes
                    .checked_add(metadata.len())
                    .context("byte count overflow")?;
                ensure!(
                    self.bytes <= MAX_BYTES,
                    "prepared input byte bound exceeded"
                );
                properties.insert("sha256".into(), json!(hash_file(&path, &metadata)?));
            } else if metadata.is_file() {
                properties.insert(
                    "hash_status".into(),
                    json!(if name == "MANIFEST" {
                        "excluded_to_avoid_graph_digest_recursion"
                    } else {
                        "protected_content_not_read"
                    }),
                );
            }
            if metadata.file_type().is_symlink() {
                let target = fs::read_link(&path)?;
                ensure!(
                    target.as_os_str().as_bytes().len() <= 4096,
                    "symlink target exceeds bound"
                );
                // Never follow a link. Absolute host paths remain unresolved declarations.
                let raw = target.to_str().context("non-UTF8 symlink target")?;
                properties.insert("target".into(), json!(raw));
                properties.insert("target_status".into(), json!("not_followed"));
            }
            self.paths.insert(name.clone());
            self.node(
                id.clone(),
                "prepared_path",
                name.clone(),
                Value::Object(properties.into_iter().collect()),
            )?;
            if relative != Path::new("") {
                self.edges.push(edge(
                    format!("file:{}", safe_rel(relative)?),
                    "contains",
                    id.clone(),
                    &name,
                    "directory_entry",
                    json!({}),
                ));
            }
            if metadata.is_dir() {
                self.walk(root, &rel, depth + 1)?;
            }
        }
        Ok(())
    }
    fn reference(
        &mut self,
        src: &str,
        target: &str,
        relation: &str,
        source: &str,
        line: usize,
        status: &str,
    ) -> Result<()> {
        let staged = map_host_path(target);
        let resolved = staged.filter(|p| self.paths.contains(p));
        let dst = if let Some(path) = &resolved {
            format!("file:{path}")
        } else {
            let id = format!("reference:{target}");
            if !self.nodes.contains_key(&id) {
                self.node(id.clone(),"unresolved_path",target.into(),json!({"path":target,"classification":if sensitive(target){"protected_runtime_requirement"}else{"external_or_runtime_requirement"},"authority":"declared_only"}))?;
            }
            id
        };
        self.edges.push(edge(src,relation,dst,source,format!("L{line}"),json!({"resolution":if resolved.is_some(){"staged_path"}else{"unresolved"},"interpretation":status})));
        Ok(())
    }
}
fn hash_file(path: &Path, before: &fs::Metadata) -> Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    let opened = file.metadata()?;
    ensure!(
        before.dev() == opened.dev()
            && before.ino() == opened.ino()
            && before.len() == opened.len(),
        "prepared file changed before read"
    );
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let len = file.read(&mut buf)?;
        if len == 0 {
            break;
        }
        total += len as u64;
        ensure!(total <= before.len(), "prepared file grew while hashing");
        hasher.update(&buf[..len]);
    }
    let after = file.metadata()?;
    ensure!(
        total == before.len()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "prepared file changed while hashing"
    );
    Ok(format!("{:x}", hasher.finalize()))
}
fn read_text(root: &Path, relative: &str) -> Result<String> {
    let path = root.join(relative);
    let m = fs::symlink_metadata(&path)?;
    ensure!(
        m.is_file() && m.len() <= MAX_TEXT,
        "text input must be bounded regular file: {relative}"
    );
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&path)?;
    let opened = file.metadata()?;
    ensure!(
        opened.ino() == m.ino() && opened.dev() == m.dev() && opened.len() == m.len(),
        "text input changed before reading"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_TEXT + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_TEXT, "text input bound exceeded");
    let after = file.metadata()?;
    ensure!(
        bytes.len() as u64 == m.len()
            && after.ctime() == m.ctime()
            && after.ctime_nsec() == m.ctime_nsec(),
        "text input changed while reading"
    );
    String::from_utf8(bytes).context("non-UTF8 text input")
}
fn map_host_path(path: &str) -> Option<String> {
    if let Some(name) = path.strip_prefix("/usr/local/bin/") {
        Some(format!("bin/{name}"))
    } else if let Some(name) = path.strip_prefix("/usr/local/sbin/") {
        Some(format!("sbin/{name}"))
    } else if let Some(name) = path.strip_prefix("/usr/local/libexec/") {
        Some(format!("libexec/{name}"))
    } else if let Some(name) = path.strip_prefix("/etc/runit/sv/") {
        Some(format!("sv/{name}"))
    } else if path.starts_with("/etc/")
        || path.starts_with("/usr/")
        || path.starts_with("/opt/")
        || path.starts_with("/var/lib/opdbus/plugin-blobs/")
    {
        Some(path.trim_start_matches('/').into())
    } else {
        None
    }
}
fn literal_paths(line: &str) -> BTreeSet<String> {
    // Extract only static absolute paths with known filesystem prefixes. No values,
    // URI userinfo, variable expansion or raw line text enter the graph.
    line.split(|c: char| c.is_whitespace() || "\"'{}()=,:;<>|".contains(c))
        .map(|s| if s.starts_with("-/") { &s[1..] } else { s })
        .filter(|s| {
            s.starts_with('/')
                && [
                    "/etc/", "/usr/", "/opt/", "/var/", "/run/", "/dev/", "/bin/", "/sbin/",
                ]
                .iter()
                .any(|p| s.starts_with(p))
        })
        .map(|s| s.trim_end_matches(['\\', ']']).to_owned())
        .filter(|s| {
            s.len() <= 450
                && !s.contains(['$', '`', '*', '?'])
                && Path::new(s)
                    .components()
                    .all(|c| !matches!(c, Component::ParentDir))
        })
        .collect()
}

fn literal_exec_program(line: &str) -> Option<&str> {
    let mut words = line.split_whitespace();
    if words.next()? != "exec" {
        return None;
    }
    let first = words.next()?.trim_matches(['\'', '"']);
    if first == "numactl" || first == "/usr/bin/numactl" {
        // Only the repository's explicit NUMA forms have a known argument shape.
        let option = words.next()?;
        if option != "--localalloc" && !option.starts_with("--interleave=") {
            return None;
        }
        let program = words.next()?.trim_matches(['\'', '"']);
        if program.starts_with('/') && !program.contains(['$', '`']) {
            Some(program)
        } else {
            None
        }
    } else if first.starts_with('/') && !first.contains(['$', '`']) {
        Some(first)
    } else {
        None
    }
}

fn prepared(
    root: &Path,
    namespace: &str,
    generated_at: &str,
    output: &Path,
) -> Result<GraphSnapshot> {
    ensure!(
        root.is_absolute(),
        "--prepared must be an explicit absolute directory"
    );
    ensure!(
        !fs::symlink_metadata(root)?.file_type().is_symlink(),
        "prepared root must not be symlink"
    );
    let root = root.canonicalize()?;
    let mut ancestor = output.parent().context("output requires parent")?;
    while !ancestor.exists() {
        ancestor = ancestor.parent().context("output parent missing")?;
    }
    let parent = ancestor.canonicalize()?;
    if parent.starts_with(&root) {
        ensure!(
            output.is_absolute()
                && output
                    .strip_prefix(&root)
                    .is_ok_and(|p| p.starts_with(GRAPH_DIR)),
            "output inside prepared tree must be under {GRAPH_DIR}"
        );
    }
    let mut b = Builder::new();
    b.walk(&root, Path::new(""), 0)?;
    ensure!(
        b.paths.contains("MANIFEST"),
        "prepared tree requires MANIFEST"
    );
    let manifest = read_text(&root, "MANIFEST")?;
    let mut inputs = BTreeMap::new();
    let mut commit = String::new();
    for line in manifest.lines() {
        if let Some((key, value)) = line.split_once(": ") {
            if key == "commit" {
                commit = value.into();
            }
            if key.ends_with("-sha256")
                && !key.starts_with("system-graph")
                && value.len() == 64
                && value.bytes().all(|x| x.is_ascii_hexdigit())
            {
                inputs.insert(format!("MANIFEST:{key}"), value.into());
            }
        }
    }
    ensure!(
        commit.len() >= 7 && commit.len() <= 64 && commit.bytes().all(|x| x.is_ascii_hexdigit()),
        "MANIFEST missing valid source commit"
    );
    let mut limitations=vec!["Prepared paths and declarations are not deployment, running processes, mounted subvolumes or live backend state.".into(),"Shell references are static declarations; dynamic variables, functions, conditional branches and generated config are not resolved.".into(),"Secrets and credential/environment files are never read; source line content is never retained.".into(),"Archives are inventoried as artifacts; their internal container filesystem is not unpacked.".into(),"The graph output subtree and MANIFEST file digest are excluded to prevent self-reference.".into()];
    let manifests = [
        ("etc/opdbus/enabled-services", "enabled"),
        ("etc/opdbus/retired-services", "retired"),
    ];
    let mut memberships: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (path, state) in manifests {
        if !b.paths.contains(path) {
            limitations.push(format!(
                "{path} absent: {state} service declarations unavailable."
            ));
            continue;
        }
        for (i, line) in read_text(&root, path)?.lines().enumerate() {
            let name = line.split('#').next().unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            ensure!(
                name.len() <= 128
                    && !name.starts_with('.')
                    && !name.ends_with('.')
                    && name
                        .bytes()
                        .all(|x| x.is_ascii_alphanumeric() || b"-_.".contains(&x)),
                "invalid staged service name"
            );
            memberships
                .entry(name.into())
                .or_default()
                .insert(state.into());
            let id = format!("service:{name}");
            if !b.nodes.contains_key(&id) {
                b.node(
                    id.clone(),
                    "runit_service",
                    name.into(),
                    json!({"authority":"prepared_declaration"}),
                )?;
            }
            b.edges.push(edge(
                id,
                &format!("declared_{state}"),
                format!("file:{path}"),
                path,
                format!("L{}", i + 1),
                json!({"runtime_state":"not_observed"}),
            ));
        }
    }
    let runs: Vec<String> = b
        .paths
        .iter()
        .filter(|p| p.starts_with("sv/") && p.ends_with("/run") && p.split('/').count() == 3)
        .cloned()
        .collect();
    for path in &runs {
        let name = path.split('/').nth(1).expect("service path");
        let service = format!("service:{name}");
        if !b.nodes.contains_key(&service) {
            b.node(
                service.clone(),
                "runit_service",
                name.into(),
                json!({"authority":"prepared_declaration"}),
            )?;
        }
        b.edges.push(edge(
            &service,
            "configured_by",
            format!("file:{path}"),
            path,
            "file",
            json!({}),
        ));
        let text = read_text(&root, path)?;
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            if words.first() == Some(&"wait_dep") && words.len() == 2 {
                let target = words[1];
                if target
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
                {
                    let id = format!("service:{target}");
                    if !b.nodes.contains_key(&id) {
                        b.node(id.clone(),"runit_service",target.into(),json!({"definition_staged":b.paths.contains(&format!("sv/{target}/run")),"authority":"prepared_reference"}))?;
                    }
                    b.edges.push(edge(
                        &service,
                        "depends_on",
                        id,
                        path,
                        format!("L{}", i + 1),
                        json!({"declaration":"wait_dep","conditional_or_scripted":true}),
                    ));
                }
            }
            for target in literal_paths(line) {
                let runs_binary = literal_exec_program(line) == Some(target.as_str())
                    && (target.starts_with("/usr/local/bin/") || target.starts_with("/usr/bin/"));
                b.reference(
                    &service,
                    &target,
                    if runs_binary {
                        "runs"
                    } else {
                        "references_path"
                    },
                    path,
                    i + 1,
                    if runs_binary {
                        "literal_exec_candidate"
                    } else {
                        "static_path_reference"
                    },
                )?;
            }
        }
    }
    let configs: Vec<String> = b
        .paths
        .iter()
        .filter(|p| {
            category(p) == "configuration"
                && !sensitive(p)
                && b.nodes[&format!("file:{p}")].attributes.get("file_type")
                    == Some(&json!("regular"))
        })
        .cloned()
        .collect();
    for path in configs {
        if fs::symlink_metadata(root.join(&path))?.len() > MAX_TEXT {
            limitations.push(format!(
                "{path}: config exceeds parsing bound; artifact hash retained."
            ));
            continue;
        }
        let text = match read_text(&root, &path) {
            Ok(t) => t,
            Err(_) => {
                limitations.push(format!("{path}: non-text config; artifact hash retained."));
                continue;
            }
        };
        for (i, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            for target in literal_paths(line) {
                b.reference(
                    &format!("file:{path}"),
                    &target,
                    "references_path",
                    &path,
                    i + 1,
                    "static_config_path",
                )?;
            }
        }
    }
    // Provenance documents are linked by category rather than copying arbitrary
    // JSON contents (which could contain future private fields).
    for (document, prefix) in [
        (
            "usr/share/op-dbus/native-manifests/tched-router.json",
            "usr/bin/zeroclaw",
        ),
        (
            "usr/share/op-dbus/ui-manifests/3tched-ui.json",
            "usr/local/share/op-dbus/dashboard/",
        ),
        (
            "var/lib/opdbus/plugin-blobs/.manifest.json",
            "var/lib/opdbus/plugin-blobs/",
        ),
    ] {
        if b.paths.contains(document) {
            let targets: Vec<_> = b
                .paths
                .iter()
                .filter(|p| p.starts_with(prefix) && *p != document)
                .cloned()
                .collect();
            for target in targets {
                b.edges.push(edge(
                    format!("file:{target}"),
                    "provenance_document",
                    format!("file:{document}"),
                    document,
                    "file",
                    json!({"validation":"document_association_only"}),
                ));
            }
        }
    }
    let catalog_path = "var/lib/opdbus/plugin-blobs/.manifest.json";
    if b.paths.contains(catalog_path) {
        let catalog: Value = serde_json::from_str(&read_text(&root, catalog_path)?)?;
        let plugins = catalog
            .get("plugins")
            .and_then(Value::as_object)
            .context("prepared catalog lacks plugins map")?;
        for (name, hash) in plugins {
            let hash = hash
                .as_str()
                .context("prepared catalog schema hash must be string")?;
            ensure!(
                name.len() <= 128
                    && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    && hash.len() == 64
                    && hash.bytes().all(|c| c.is_ascii_hexdigit()),
                "invalid prepared catalog plugin entry"
            );
            let blob = format!("var/lib/opdbus/plugin-blobs/{name}.{}.blob", &hash[..16]);
            ensure!(
                b.paths.contains(&blob),
                "prepared catalog blob missing for {name}"
            );
            let id = format!("plugin:{name}");
            b.node(id.clone(), "sealed_plugin", name.clone(), json!({"schema_hash":hash,"authority":"prepared_catalog","live_presence_verified":false}))?;
            b.edges.push(edge(
                id,
                "sealed_in",
                format!("file:{blob}"),
                catalog_path,
                format!("/plugins/{name}"),
                json!({"validation":"manifest_pinned_path"}),
            ));
        }
    }
    for (name, states) in memberships {
        let node = b
            .nodes
            .get_mut(&format!("service:{name}"))
            .expect("membership node");
        node.attributes.insert("declarations".into(), json!(states));
        if states.len() > 1 {
            limitations.push(format!(
                "Service {name} appears in enabled and retired declarations; conflict preserved."
            ));
        }
    }
    b.edges.sort_by_key(|e| {
        (
            e.src.clone(),
            e.relation.clone(),
            e.dst.clone(),
            e.evidence[0].source.clone(),
            e.evidence[0].locator.clone(),
        )
    });
    limitations.sort();
    limitations.dedup();
    Ok(GraphSnapshot{format_version:1,namespace:namespace.into(),provenance:GraphProvenance{kind:GraphProvenanceKind::PreparedRelease,source_commit:commit,release_id:Some(namespace.trim_start_matches("release:").into()),generated_at:generated_at.into(),inputs,coverage:vec!["All actual prepared files, directories and symlinks (bounded, links not followed)".into(),"Runit definitions, explicit service lifecycle manifests, static exec/path/dependency references".into(),"Configuration, container archives, UI/native and sealed catalog provenance artifacts".into()],limitations},nodes:b.nodes.into_values().collect(),edges:b.edges})
}

#[derive(Deserialize)]
struct LegacyGraph {
    nodes: Vec<LegacyNode>,
    edges: Vec<LegacyEdge>,
}
#[derive(Deserialize)]
struct LegacyNode {
    id: String,
    label: String,
    props: Value,
}
#[derive(Deserialize)]
struct LegacyEdge {
    src: String,
    rel: String,
    dst: String,
    props: Value,
}
fn source_graph(path: &Path, namespace: &str, generated_at: &str) -> Result<GraphSnapshot> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && metadata.len() <= MAX_JSON as u64,
        "source graph requires bounded regular file"
    );
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_JSON as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= MAX_JSON, "source graph exceeds bound");
    let input: LegacyGraph = serde_json::from_slice(&bytes)?;
    ensure!(
        input.nodes.len() <= MAX_ENTRIES && input.edges.len() <= 180_000,
        "source graph entry bound exceeded"
    );
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();
    let mut commit = None;
    let mut worktree_dirty = None;
    for node in input.nodes {
        ensure!(
            node.id.len() <= 450 && node.props.is_object(),
            "invalid source node"
        );
        if node.id == "meta:crate-graph" {
            commit = node
                .props
                .get("commit")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let Some(value) = node.props.get("worktree_dirty") {
                worktree_dirty = Some(
                    value
                        .as_bool()
                        .context("source worktree_dirty must be boolean")?,
                );
            }
        }
        let attributes = attrs(node.props.clone());
        if let Some(existing) = nodes.get_mut(&node.id) {
            if existing.kind == node.label && existing.attributes == attributes {
                continue;
            }
            ensure!(
                node.id.starts_with("ext:")
                    && existing.kind == "ext_crate"
                    && node.label == "ext_crate",
                "conflicting duplicate source node {}",
                node.id
            );
            let original = existing.attributes.remove("req");
            let values = existing
                .attributes
                .entry("requirements".into())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .context("invalid merged requirements")?;
            if let Some(v) = original {
                values.push(v);
            }
            let req = node
                .props
                .get("req")
                .context("external duplicate lacks req")?
                .clone();
            if !values.contains(&req) {
                values.push(req);
            }
            values.sort_by_key(Value::to_string);
            ensure!(
                node.props.as_object().is_some_and(|o| o.len() == 1),
                "external duplicate has unsupported properties"
            );
        } else {
            nodes.insert(
                node.id.clone(),
                GraphNode {
                    id: node.id.clone(),
                    kind: node.label,
                    label: node.id,
                    attributes,
                },
            );
        }
    }
    let mut edges = Vec::new();
    for item in input.edges {
        ensure!(
            nodes.contains_key(&item.src) && nodes.contains_key(&item.dst),
            "source edge endpoint absent"
        );
        ensure!(
            item.props.is_object(),
            "source edge attributes must be object"
        );
        let source = nodes
            .get(&item.src)
            .and_then(|n| n.attributes.get("file").or_else(|| n.attributes.get("src")))
            .and_then(Value::as_str)
            .unwrap_or("scripts/crate-graph.sh");
        let assertion = if item.rel == "imports" || item.rel == "runs_bin" {
            GraphAssertion::Inferred
        } else {
            GraphAssertion::Declared
        };
        let mut e = edge(
            item.src.clone(),
            &item.rel,
            item.dst,
            source,
            "source_graph",
            item.props,
        );
        e.evidence[0].assertion = assertion;
        edges.push(e);
    }
    edges.sort_by_key(|e| {
        (
            e.src.clone(),
            e.relation.clone(),
            e.dst.clone(),
            serde_json::to_string(&e.attributes).unwrap_or_default(),
        )
    });
    edges.dedup_by(|a, b| {
        a.src == b.src && a.relation == b.relation && a.dst == b.dst && a.attributes == b.attributes
    });
    let commit = commit.context("source graph lacks meta:crate-graph commit")?;
    ensure!(
        commit.len() >= 7 && commit.len() <= 64 && commit.bytes().all(|c| c.is_ascii_hexdigit()),
        "invalid source commit"
    );
    let mut limitations = vec![
        "Source inventory does not prove sealed, staged, deployed or running existence.".into(),
        "External dependency requirements merged; scanner run/import edges remain inferred.".into(),
    ];
    match worktree_dirty {
        Some(true) => limitations.push("Source includes uncommitted working-tree changes; source_commit alone does not identify the scanned source.".into()),
        None => limitations.push("Source worktree cleanliness is unknown: legacy scanner supplied no dirty marker.".into()),
        Some(false) => {}
    }
    Ok(GraphSnapshot {
        format_version: 1,
        namespace: namespace.into(),
        provenance: GraphProvenance {
            kind: GraphProvenanceKind::Source,
            source_commit: commit,
            release_id: None,
            generated_at: generated_at.into(),
            inputs: BTreeMap::from([
                ("source_graph_sha256".into(), digest(&bytes)),
                (
                    "source_worktree_dirty".into(),
                    worktree_dirty.map_or_else(|| "unknown".into(), |v| v.to_string()),
                ),
            ]),
            coverage: vec![
                "Existing Cargo/source graph declarations and scanner inferences".into(),
            ],
            limitations,
        },
        nodes: nodes.into_values().collect(),
        edges,
    })
}
fn atomic_write(output: &Path, bytes: &[u8]) -> Result<()> {
    ensure!(bytes.len() <= MAX_JSON, "encoded graph bound exceeded");
    let parent = output.parent().context("output requires parent")?;
    // Parent must already exist: callers explicitly stage the output directory.
    ensure!(
        fs::symlink_metadata(parent)?.is_dir(),
        "output parent must be directory"
    );
    for p in parent.ancestors() {
        ensure!(
            !fs::symlink_metadata(p)?.file_type().is_symlink(),
            "output parent symlink not permitted"
        );
    }
    if let Ok(metadata) = fs::symlink_metadata(output) {
        ensure!(metadata.is_file(), "output must be regular file");
    }
    let temp = parent.join(format!(".system-graph-{}.tmp", std::process::id()));
    let mut created = false;
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        created = true;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temp, output)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests;

fn merge_edges(edges: Vec<GraphEdge>) -> Result<Vec<GraphEdge>> {
    let mut groups: BTreeMap<(String, String, String), Vec<GraphEdge>> = BTreeMap::new();
    for edge in edges {
        groups
            .entry((edge.src.clone(), edge.relation.clone(), edge.dst.clone()))
            .or_default()
            .push(edge);
    }
    let mut output = Vec::new();
    for (_, items) in groups {
        let mut first = items[0].clone();
        let mut occurrences = Vec::new();
        let mut records: BTreeMap<(String, GraphAssertion), BTreeSet<String>> = BTreeMap::new();
        for edge in &items {
            let properties = serde_json::to_value(&edge.attributes)?;
            if !occurrences.contains(&properties) {
                occurrences.push(properties);
            }
            for evidence in &edge.evidence {
                records
                    .entry((evidence.source.clone(), evidence.assertion))
                    .or_default()
                    .insert(evidence.locator.clone());
            }
        }
        if occurrences.len() > 1 {
            occurrences.sort_by_key(Value::to_string);
            first.attributes = attrs(json!({"occurrences":occurrences}));
        }
        first.evidence = records
            .into_iter()
            .map(|((source, assertion), locators)| GraphEvidence {
                source,
                assertion,
                locator: locators.into_iter().collect::<Vec<_>>().join(","),
            })
            .collect();
        ensure!(
            first.evidence.len() <= 32 && first.evidence.iter().all(|e| e.locator.len() <= 2048),
            "edge evidence bound exceeded"
        );
        output.push(first);
    }
    Ok(output)
}
