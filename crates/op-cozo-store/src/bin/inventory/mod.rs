//! Bounded inventory of explicitly supplied prepared bytes, not deployed state.
//!
//! Shell dependency analysis preserves quoting, command substitutions and
//! backslash continuations without executing them. Supported path-producing
//! forms are literals/defaults/aliases, bounded function arguments, lexical
//! readlink -f -- $0/dirname/CDPATH+pwd wrappers and mktemp template directories.
//! The unix:path= default/prefix-removal pair tracks external socket paths only.
//! Other computed executable/config/include paths and unknown path arguments
//! remain incomplete. Source-provenance JSON is recognized structurally; mixed
//! or unknown schemas keep ordinary configuration scanning.
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
const SERVICE_MANIFEST_DIR: &str = "usr/share/op-dbus/etc/opdbus";
const CATALOG_DIR: &str = "usr/share/op-dbus/plugin-blobs";

// These are directory ownership boundaries, not another service inventory.
const RELEASE_ROOTS: &[(&str, &str, bool)] = &[
    ("/usr/share/op-dbus", "usr/share/op-dbus", false),
    ("/usr/local/libexec/op-dbus/helpers", "libexec/3tched", false),
    ("/usr/local/libexec/op-dbus/bin", "bin", false),
    ("/usr/local/libexec/op-dbus/threetched", "usr/bin", false),
    ("/usr/local/libexec/op-dbus/sv", "sv", false),
    (
        "/usr/local/share/op-dbus/dashboard-release",
        "usr/local/share/op-dbus/dashboard",
        true,
    ),
    ("/opt/opdbus", "opt/opdbus", false),
    ("/opt/op-dbus/execution-python", "opt/op-dbus/execution-python", false),
];

fn beneath(path: &str, parent: &str) -> bool {
    path == parent || path.strip_prefix(parent).is_some_and(|rest| rest.starts_with('/'))
}

fn valid_service(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && !name.ends_with('.')
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
}

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
        || path.starts_with("usr/share/op-dbus/etc/")
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
    release_roots: BTreeMap<String, String>,
    blockers: BTreeMap<String, BTreeSet<String>>,
    argument_paths: BTreeMap<String, BTreeMap<String, BTreeSet<String>>>,
    template_roots: BTreeMap<String, BTreeSet<String>>,
    conservative: BTreeSet<String>,
}
impl Builder {
    fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: Vec::new(),
            paths: BTreeSet::new(),
            bytes: 0,
            release_roots: BTreeMap::new(),
            blockers: BTreeMap::new(),
            argument_paths: BTreeMap::new(),
            template_roots: BTreeMap::new(),
            conservative: BTreeSet::new(),
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
                json!({"path":name,"file_type":file_type,"size":metadata.len(),"mode":metadata.mode() & 0o7777,"uid":metadata.uid(),"gid":metadata.gid(),"category":category(&name),"authority":"prepared_release","release_target":null,"consumers":[]}),
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
                // Read only the literal destination; later graph resolution is lexical, never a filesystem follow.
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
    fn release_targets(&mut self) -> Result<()> {
        let mut roots: Vec<(String, String, bool)> = RELEASE_ROOTS
            .iter()
            .filter(|(_, source, _)| self.paths.contains(*source))
            .map(|(path, source, noexec)| ((*path).into(), (*source).into(), *noexec))
            .collect();
        let emqx: Vec<_> = self.paths.iter().filter(|path| {
            path.starts_with("opt/emqx-") && path.split('/').count() == 2
                && self.nodes[&format!("file:{path}")].attributes["file_type"] == "directory"
        }).collect();
        ensure!(emqx.len() <= 1, "multiple prepared EMQX installation roots");
        if let Some(source) = emqx.first() {
            for (name, target) in [
                ("etc", "/usr/share/op-dbus/emqx-etc"),
                ("data", "/var/lib/emqx"),
                ("log", "/var/log/emqx"),
            ] {
                let node = self.nodes.get(&format!("file:{source}/{name}"))
                    .with_context(|| format!("EMQX {name} separation missing in {source}"))?;
                ensure!(
                    node.attributes["file_type"] == "symlink" && node.attributes["target"] == target,
                    "EMQX {source}/{name} must be a literal symlink to {target}"
                );
            }
            roots.push(("/opt/emqx".into(), (*source).clone(), false));
        }
        for (target, source, require_noexec) in roots {
            ensure!(
                self.nodes[&format!("file:{source}")].attributes["file_type"] == "directory",
                "release target source must be a real directory: {source}"
            );
            if source == "usr/bin" {
                ensure!(
                    self.paths.iter().filter(|p| beneath(p, &source) && *p != &source)
                        .all(|p| p == "usr/bin/threetched"),
                    "threetched target contains unrelated usr/bin entries"
                );
            }
            let noexec = !self.nodes.values().any(|node| {
                node.kind == "prepared_path"
                    && node.attributes["file_type"] == "regular"
                    && node.attributes["mode"].as_u64().is_some_and(|mode| mode & 0o111 != 0)
                    && node.attributes["path"].as_str().is_some_and(|path| beneath(path, &source))
            });
            ensure!(!require_noexec || noexec, "non-executable release target contains executable files: {source}");
            self.node(
                format!("target:{target}"), "release_target", target.clone(),
                json!({"path":target,"source":source,"noexec":noexec}),
            )?;
            self.release_roots.insert(target, source);
        }
        for path in &self.paths {
            let target = self.release_roots.iter()
                .find(|(_, source)| beneath(path, source)).map(|(target, _)| target);
            self.nodes.get_mut(&format!("file:{path}")).expect("prepared path")
                .attributes.insert("release_target".into(), json!(target));
        }
        Ok(())
    }

    fn staged_path(&self, target: &str) -> Option<String> {
        self.release_roots.iter().find_map(|(host, source)| {
            if beneath(target, host) {
                Some(format!("{source}{}", &target[host.len()..]))
            } else {
                None
            }
        }).or_else(|| map_host_path(target))
    }

    fn host_path(&self, source: &str) -> Option<String> {
        self.release_roots.iter().find_map(|(host, root)| {
            beneath(source, root).then(|| format!("{host}{}", &source[root.len()..]))
        })
    }

    fn blocker(&mut self, source: &str, line: usize, reason: &str) {
        // Store locations and classifications only, never shell text or values.
        self.blockers.entry(format!("file:{source}")).or_default()
            .insert(format!("{source}:L{line}: {reason}"));
    }

    fn dependency(&mut self, src: &str, name: &str, starts: bool, source: &str, line: usize) -> Result<()> {
        let id = format!("service:{name}");
        if !self.nodes.contains_key(&id) {
            self.node(id.clone(), "runit_service", name.into(),
                json!({"authority":"prepared_reference","release_owned":false,"definition_staged":false,"declarations":[],"deps":[],"starts":[]}))?;
        }
        self.edges.push(edge(src, "depends_on", &id, source, format!("L{line}"),
            json!({"conditional_or_scripted":true})));
        if starts {
            self.edges.push(edge(src, "starts", id, source, format!("L{line}"),
                json!({"conditional_or_scripted":true})));
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
        let staged = self.staged_path(target);
        let resolved = staged.filter(|p| self.paths.contains(p));
        if resolved.is_none() && self.release_roots.keys().any(|root| beneath(target, root))
            && !beneath(target, "/usr/share/op-dbus/graphs")
        {
            self.blocker(source, line, "referenced release path is absent from prepared content");
        }
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
        if let Some((_, bundle)) = self.release_roots.iter().find(|(host, _)| {
            host.starts_with("/opt/") && beneath(target, host)
        }) {
            self.edges.push(edge(src, "references_path", format!("file:{bundle}"),
                source, format!("L{line}"), json!({"interpretation":"complete_immutable_runtime_bundle"})));
        }
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
    if let Some(name) = path.strip_prefix("/etc/sv/") {
        Some(format!("sv/{name}"))
    } else if path.starts_with("/etc/") || path.starts_with("/usr/") || path.starts_with("/opt/") {
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

fn shell_word(word: &str) -> &str {
    word.trim_matches(['\'', '"', ';', '\\'])
}

// Keep shell words intact: quoted whitespace and nested substitutions are not
// argument boundaries. Substitutions are scanned for dependencies, never run.
// This is a bounded dependency lexer, not a shell interpreter or syntax checker.
fn shell_commands(line: &str) -> Result<Vec<Vec<&str>>> {
    fn group_end(bytes: &[u8], mut i: usize, close: u8, depth: usize) -> Result<usize> {
        ensure!(depth <= 32, "shell substitution nesting exceeds inventory bound");
        let mut quote = 0;
        while i < bytes.len() {
            let ch = bytes[i];
            if ch == b'\\' && quote != b'\'' {
                i += 2;
                continue;
            }
            if ch == quote {
                quote = 0;
            } else if quote != b'\'' && ch == b'$' && matches!(bytes.get(i + 1), Some(b'(' | b'{')) {
                let end = if bytes[i + 1] == b'(' { b')' } else { b'}' };
                i = group_end(bytes, i + 2, end, depth + 1)?;
            } else if quote == 0 {
                if ch == close {
                    return Ok(i);
                }
                if matches!(ch, b'\'' | b'"') {
                    quote = ch;
                } else if ch == b'(' {
                    i = group_end(bytes, i + 1, b')', depth + 1)?;
                }
            }
            i += 1;
        }
        // A command may continue on the following physical line (e.g. a pipe
        // inside $()). Keep its operands, but never invent its output.
        Ok(bytes.len())
    }
    fn lex<'a>(line: &'a str, depth: usize, commands: &mut Vec<Vec<&'a str>>) -> Result<()> {
        ensure!(depth <= 32, "shell substitution nesting exceeds inventory bound");
        let bytes = line.as_bytes();
        let mut words = Vec::new();
        let mut start = None;
        let mut quote = 0;
        let mut i = 0;
        while i < bytes.len() {
            let ch = bytes[i];
            if ch == b'\\' && quote != b'\'' {
                start.get_or_insert(i);
                i += 2;
                continue;
            }
            if ch == quote {
                quote = 0;
            } else if quote != b'\'' && ch == b'$' && matches!(bytes.get(i + 1), Some(b'(' | b'{')) {
                start.get_or_insert(i);
                let close = if bytes[i + 1] == b'(' { b')' } else { b'}' };
                let end = group_end(bytes, i + 2, close, depth + 1)?;
                if close == b')' && bytes.get(i + 2) != Some(&b'(') {
                    lex(&line[i + 2..end], depth + 1, commands)?;
                }
                i = end;
            } else if quote == 0 {
                if matches!(ch, b'\'' | b'"') {
                    quote = ch;
                } else if ch.is_ascii_whitespace() || b";|&()".contains(&ch) {
                    if let Some(begin) = start.take() {
                        words.push(&line[begin..i]);
                    }
                    if !ch.is_ascii_whitespace() && !words.is_empty() {
                        commands.push(std::mem::take(&mut words));
                    }
                    i += 1;
                    continue;
                }
            }
            start.get_or_insert(i.min(bytes.len()));
            i += 1;
        }
        if let Some(begin) = start {
            if begin < bytes.len() {
                words.push(&line[begin..]);
            }
        }
        if !words.is_empty() {
            commands.push(words);
        }
        Ok(())
    }
    let mut commands = Vec::new();
    lex(line, 0, &mut commands)?;
    Ok(commands)
}

// Remove only shell backslash-newline continuations. Preserve physical start
// lines for evidence and avoid treating printf's continued data as a command.
fn shell_lines(text: &str) -> Vec<(usize, std::borrow::Cow<'_, str>)> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    let mut first = 1;
    let mut single = false;
    for (index, raw) in text.lines().enumerate() {
        let mut escaped = false;
        let mut double = false;
        for (offset, ch) in raw.char_indices() {
            if escaped {
                escaped = false;
            } else if ch == '\\' && !single {
                escaped = true;
            } else if ch == '"' && !single {
                double = !double;
            } else if ch == '\'' && !double {
                single = !single;
            } else if ch == '#' && !single && !double
                && (offset == 0 || raw.as_bytes()[offset - 1].is_ascii_whitespace())
            {
                break;
            }
        }
        if pending.is_empty() {
            first = index + 1;
        }
        if escaped && !single {
            pending.push_str(&raw[..raw.len() - 1]);
        } else if pending.is_empty() {
            lines.push((first, std::borrow::Cow::Borrowed(raw)));
        } else {
            pending.push_str(raw);
            lines.push((first, std::borrow::Cow::Owned(std::mem::take(&mut pending))));
        }
    }
    if !pending.is_empty() {
        lines.push((first, std::borrow::Cow::Owned(pending)));
    }
    lines
}

fn variable_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

fn normalize_absolute(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }
    let mut result = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return None;
                }
            }
            Component::Normal(part) => result.push(part),
            Component::Prefix(_) => return None,
        }
    }
    result.to_str().map(String::from)
}

// Ignore multiline single-quoted awk/jq/data bodies, not their trailing
// command. Quote tracking never interprets the data as executable shell.
fn shell_code<'a>(line: &'a str, single: &mut bool) -> &'a str {
    let mut double = false;
    let mut escaped = false;
    let mut start = if *single { line.len() } else { 0 };
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if !*single => escaped = true,
            '"' if !*single => double = !double,
            '\'' if !double => {
                *single = !*single;
                if !*single && start == line.len() {
                    start = index + 1;
                }
            }
            '#' if !*single && !double
                && (index == 0 || line.as_bytes()[index - 1].is_ascii_whitespace()) =>
            {
                return if start < index { &line[start..index] } else { "" };
            }
            _ => {}
        }
    }
    &line[start..]
}

// Resolve only literal assignments, simple aliases and literal path defaults.
// This is a union of declared alternatives, never execution of the script.
fn declared_paths(word: &str, bindings: &BTreeMap<String, BTreeSet<String>>) -> BTreeSet<String> {
    let word = shell_word(word);
    let mut result = BTreeSet::new();
    if let Some(command) = word.strip_prefix("$(").and_then(|word| word.strip_suffix(')')) {
        if let Some(arg) = command.strip_prefix("dirname -- ").or_else(|| command.strip_prefix("dirname ")) {
            let Ok(commands) = shell_commands(command) else { return result };
            if commands.last().and_then(|words| words.last()).copied() != Some(arg.trim()) {
                return result;
            }
            return declared_paths(arg, bindings).into_iter().filter_map(|path| {
                Path::new(&path).parent().and_then(Path::to_str).map(String::from)
            }).collect();
        }
        if let Some(arg) = command.strip_prefix("readlink -f -- ").filter(|arg| shell_word(arg) == "$0") {
            return declared_paths(arg, bindings);
        }
        if let Some(arg) = command.strip_prefix("CDPATH= cd -- ").and_then(|word| word.strip_suffix(" && pwd -P")) {
            return declared_paths(arg, bindings);
        }
        if command.starts_with("mktemp ") {
            let Ok(commands) = shell_commands(command) else { return result };
            let Some(words) = commands.first().filter(|_| commands.len() == 1) else { return result };
            let template = match words.as_slice() {
                ["mktemp", template] | ["mktemp", "-d", template] => *template,
                _ => return result,
            };
            return declared_paths(template, bindings).into_iter().filter_map(|path| {
                Path::new(&path).parent().and_then(Path::to_str).map(String::from)
            }).collect();
        }
        return result;
    }
    if word.starts_with('/') {
        if word.contains([';', '|', '&', '(', ')', '"', '\'', '\\']) {
            return result;
        }
        if let Some(at) = word.find(['$', '`', '*', '?']) {
            if let Some(end) = word[..at].rfind('/') {
                if end > 0 {
                    result.insert(word[..end].into());
                }
            }
        } else if !word.contains("/../") && !word.ends_with("/..") {
            if let Some(path) = normalize_absolute(Path::new(word)) {
                result.insert(path);
            }
        }
        return result;
    }
    let Some(rest) = word.strip_prefix('$') else { return result };
    let (name, suffix, default) = if let Some(braced) = rest.strip_prefix('{') {
        let Some((inner, suffix)) = braced.split_once('}') else { return result };
        if let Some(paths) = bindings.get(inner).filter(|_| inner.ends_with("#unix:path=") && suffix.is_empty()) {
            return paths.clone();
        }
        if let Some((name, default)) = inner.split_once(":-").or_else(|| inner.split_once('-')) {
            (name, suffix, Some(default))
        } else {
            (inner, suffix, None)
        }
    } else {
        let end = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(rest.len());
        (&rest[..end], &rest[end..], None)
    };
    if !variable_name(name) {
        return result;
    }
    if let Some(paths) = bindings.get(name) {
        result.extend(paths.iter().cloned());
    }
    if let Some(default) = default {
        result.extend(declared_paths(default, bindings));
    }
    let suffix = suffix.trim_matches(['\'', '"']);
    if !suffix.is_empty() {
        result = result.into_iter().flat_map(|path| {
            declared_paths(&format!("{path}{suffix}"), bindings)
        }).collect();
    }
    result
}

// Recognize the captured-source document's structure, not its filename or
// location. Citation source_path fields belong to the upstream repository;
// they are not local runtime includes. Unknown/mixed schemas stay configuration.
fn source_provenance(text: &str) -> bool {
    let Ok(Value::Object(document)) = serde_json::from_str::<Value>(text) else { return false };
    let relative = |value: &Value| value.as_str().is_some_and(|path| {
        !path.is_empty() && Path::new(path).components().all(|part| matches!(part, Component::Normal(_)))
    });
    let hash = |value: &Value, length| value.as_str().is_some_and(|hash| {
        hash.len() == length && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    let origin = |entry: &serde_json::Map<String, Value>| {
        entry.get("repository").and_then(Value::as_str).is_some_and(|url| url.starts_with("https://"))
            && entry.get("commit").is_some_and(|value| hash(value, 40))
    };
    let captured_file = |entry: &serde_json::Map<String, Value>| {
        entry.get("source_path").is_some_and(relative)
            && entry.get("sha256").is_some_and(|value| hash(value, 64))
            && entry.get("upstream_sha256").is_none_or(|value| hash(value, 64))
            && entry.get("adaptation").is_none_or(Value::is_string)
    };
    match document.get("format").and_then(Value::as_str) {
        Some("3tched-ui-authoring-sources/1") => {
            return document.keys().all(|key| matches!(key.as_str(), "format" | "captured_at" | "files"))
                && document.get("captured_at").is_some_and(Value::is_string)
                && document.get("files").and_then(Value::as_array).is_some_and(|files| {
                    !files.is_empty() && files.iter().all(|file| file.as_object().is_some_and(|entry| {
                        entry.keys().all(|key| matches!(key.as_str(),
                            "source" | "repository" | "ref" | "commit" | "source_path" | "path"
                            | "sha256" | "upstream_sha256" | "adaptation"))
                            && origin(entry) && captured_file(entry)
                            && entry.get("source").is_some_and(Value::is_string)
                            && entry.get("ref").is_some_and(Value::is_string)
                            && entry.get("path").is_some_and(relative)
                    }))
                });
        }
        Some("3tched-skill-packaging-changes/1") => {
            return document.keys().all(|key| matches!(key.as_str(), "format" | "skill" | "changes"))
                && document.get("skill").is_some_and(Value::is_string)
                && document.get("changes").and_then(Value::as_array).is_some_and(|changes| {
                    !changes.is_empty() && changes.iter().all(|change| change.as_object().is_some_and(|entry| {
                        entry.keys().all(|key| matches!(key.as_str(),
                            "repository" | "commit" | "source_path" | "packaged_path" | "upstream_sha256"
                            | "sha256" | "adaptation" | "replacements"))
                            && origin(entry) && captured_file(entry)
                            && entry.get("packaged_path").is_some_and(relative)
                            && entry.get("upstream_sha256").is_some_and(|value| hash(value, 64))
                            && entry.get("adaptation").is_some_and(Value::is_string)
                            && entry.get("replacements").and_then(Value::as_array).is_some_and(|replacements| {
                                !replacements.is_empty() && replacements.iter().all(|replacement| {
                                    replacement.as_object().is_some_and(|fields| {
                                        fields.keys().all(|key| matches!(key.as_str(), "from" | "to" | "occurrences"))
                                            && fields.get("from").is_some_and(Value::is_string)
                                            && fields.get("to").is_some_and(Value::is_string)
                                            && fields.get("occurrences").and_then(Value::as_u64).is_some_and(|count| count > 0)
                                    })
                                })
                            })
                    }))
                });
        }
        Some(_) => return false,
        None => {}
    }
    if !document.keys().all(|key| matches!(key.as_str(), "repository" | "commit" | "source_path" | "license_status" | "files"))
        || !document.get("repository").and_then(Value::as_str).is_some_and(|url| url.starts_with("https://"))
        || !document.get("commit").is_some_and(|value| hash(value, 40))
        || !document.get("source_path").is_some_and(relative)
        || !document.get("license_status").is_none_or(Value::is_string)
    {
        return false;
    }
    document.get("files").and_then(Value::as_array).is_some_and(|files| {
        !files.is_empty() && files.iter().all(|file| {
            file.as_object().is_some_and(|entry| {
                entry.keys().all(|key| matches!(key.as_str(), "path" | "source_path" | "sha256" | "upstream_sha256" | "adaptation"))
                    && entry.get("path").is_some_and(relative)
                    && entry.get("source_path").is_some_and(relative)
                    && entry.get("sha256").is_some_and(|value| hash(value, 64))
                    && entry.get("upstream_sha256").is_none_or(|value| hash(value, 64))
                    && entry.get("adaptation").is_none_or(Value::is_string)
            })
        })
    })
}

fn bind_shell_command(
    words: &[&str],
    bindings: &mut BTreeMap<String, BTreeSet<String>>,
) {
    if words.first() == Some(&"unset") {
        for name in words.iter().skip(1) {
            bindings.remove(shell_word(name));
            bindings.remove(&format!("{}#unix:path=", shell_word(name)));
        }
        return;
    }
    if words.starts_with(&["set", "--"]) {
        bindings.retain(|name, _| !name.bytes().all(|c| c.is_ascii_digit()));
        return;
    }
    let assignment_words = if matches!(words.first(), Some(&"export") | Some(&"local")) {
        &words[1..]
    } else if words.iter().all(|word| word.split_once('=').is_some_and(|(name, _)| variable_name(name))) {
        words
    } else {
        // Prefix assignments belong only to that command's environment.
        &[]
    };
    for word in assignment_words {
        let Some((name, value)) = word.split_once('=').filter(|(name, _)| variable_name(name)) else { break };
        let paths = declared_paths(value, bindings);
        if paths.is_empty() {
            // An unknown overwrite cannot borrow an earlier known pathname.
            bindings.remove(name);
        } else {
            bindings.entry(name.into()).or_default().extend(paths);
        }
        let key = format!("{name}#unix:path=");
        if let Some(path) = shell_word(value).strip_prefix("unix:path=") {
            let paths = declared_paths(path, bindings);
            bindings.insert(key, paths);
        } else {
            bindings.remove(&key);
        }
    }
    if words.first() == Some(&":") {
        for word in &words[1..] {
            if let Some((name, path)) = shell_word(word).strip_prefix("${")
                .and_then(|word| word.strip_suffix('}'))
                .and_then(|word| word.split_once(":=unix:path="))
                .filter(|(name, _)| variable_name(name))
            {
                let paths = declared_paths(path, bindings);
                bindings.entry(format!("{name}#unix:path=")).or_default().extend(paths);
            }
        }
    }
}

fn service_argument(word: &str) -> Option<&str> {
    let word = shell_word(word);
    let name = ["/etc/sv/", "/usr/local/libexec/op-dbus/sv/"]
        .iter()
        .find_map(|prefix| word.strip_prefix(prefix))
        .unwrap_or(word);
    valid_service(name).then_some(name)
}

fn sv_call<'a>(words: &'a [&'a str]) -> Option<(&'a str, &'a [&'a str])> {
    let start = words.iter().position(|word| {
        !matches!(*word, "until" | "while" | "if" | "then" | "do" | "!" | "exec")
    })?;
    if !matches!(words[start], "sv" | "/usr/bin/sv" | "/bin/sv") {
        return None;
    }
    let mut command = start + 1;
    while words.get(command).is_some_and(|word| word.starts_with('-')) {
        command += if words[command] == "-w" { 2 } else { 1 };
    }
    Some((*words.get(command)?, &words[command + 1..]))
}

fn sv_starts(command: &str) -> bool {
    matches!(command, "start" | "up" | "u" | "once" | "o" | "restart" | "force-restart" | "try-restart" | "force-start")
}

fn executable_position(words: &[&str]) -> Option<usize> {
    let mut position = 0;
    while words.get(position).is_some_and(|word| {
        matches!(*word, "exec" | "if" | "then" | "until" | "while" | "!" | "do")
    }) {
        position += 1;
    }
    loop {
        // Assignment words are not executables. Their values and nested
        // substitutions are still scanned; a following command is not skipped.
        while words.get(position).is_some_and(|word| {
            word.split_once('=').is_some_and(|(name, _)| {
                variable_name(name) && !name.as_bytes()[0].is_ascii_digit()
            })
        }) {
            position += 1;
        }
        match shell_word(words.get(position)?) {
            "env" | "/usr/bin/env" => {
                position += 1;
                while words.get(position).is_some_and(|word| word.starts_with('-') || word.contains('=')) {
                    position += if words[position] == "-u" { 2 } else { 1 };
                }
            }
            "chpst" | "/usr/bin/chpst" => {
                position += 1;
                while words.get(position).is_some_and(|word| word.starts_with('-')) {
                    position += if matches!(words[position], "-u" | "-U" | "-e" | "-m" | "-n") { 2 } else { 1 };
                }
            }
            "numactl" | "/usr/bin/numactl" => {
                position += 1;
                while words.get(position).is_some_and(|word| word.starts_with("--")) {
                    position += 1;
                }
            }
            _ => return Some(position),
        }
    }
}

fn config_paths(b: &mut Builder, source: &str, line_number: usize, line: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    let Some((key, value)) = line.split_once('=').or_else(|| line.split_once(':'))
        .or_else(|| line.split_once(char::is_whitespace)) else {
        return paths;
    };
    let key = key.trim().trim_matches(['\'', '"']);
    if !(matches!(key, "include" | "include_dir" | "path" | "root" | "directory" | "file")
        || key.ends_with("_path") || key.ends_with("_file") || key.ends_with("_dir"))
    {
        return paths;
    }
    let Some(host) = b.host_path(source) else { return paths };
    let roots = b.template_roots.get(source).cloned().unwrap_or_else(|| {
        Path::new(&host).parent().and_then(Path::to_str)
            .map(String::from).into_iter().collect()
    });
    for word in value.split(|c: char| c.is_whitespace() || "\"'[],;".contains(c)).filter(|word| !word.is_empty()) {
        if word.starts_with('#') {
            break;
        }
        if word == "=" || word.contains("://") {
            continue;
        }
        if word.contains(['$', '`', '*', '?']) {
            let bounded = declared_paths(word, &BTreeMap::new());
            if bounded.is_empty() {
                b.blocker(source, line_number, "unresolved dynamic configuration reference");
            } else {
                paths.extend(bounded);
                b.conservative.insert(format!("{source}:L{line_number}: dynamic configuration reference consumes its complete literal directory prefix."));
            }
        } else {
            for root in &roots {
                if let Some(path) = normalize_absolute(&Path::new(root).join(word)) {
                    paths.insert(path);
                }
            }
        }
    }
    paths
}

// Standalone, non-nested POSIX functions get call-site path bindings, isolated
// from the containing script's positional arguments. Non-path data is not a
// dependency. Unknown values still block when consumed as executable/config/
// include paths, or when any call leaves a known path argument unbounded.
fn shell_function(line: &str) -> Option<&str> {
    let name = line.strip_suffix('{')?.trim().strip_suffix("()")?.trim();
    (variable_name(name) && !name.as_bytes()[0].is_ascii_digit()).then_some(name)
}

fn function_arguments(
    b: &mut Builder,
    source: &str,
    lines: &[(usize, std::borrow::Cow<'_, str>)],
) -> Result<BTreeMap<String, BTreeMap<String, BTreeSet<String>>>> {
    let mut functions = BTreeMap::new();
    let mut incomplete = BTreeMap::<String, (usize, BTreeSet<usize>)>::new();
    let mut quoted = false;
    for (_, raw) in lines {
        if let Some(name) = shell_function(shell_code(raw, &mut quoted).trim()) {
            if name != "wait_dep" {
                functions.insert(name.to_owned(), BTreeMap::new());
            }
        }
    }
    quoted = false;
    let mut bindings = b.argument_paths.get(source).cloned().unwrap_or_default();
    if let Some(host) = b.host_path(source) {
        bindings.insert("0".into(), BTreeSet::from([host]));
    }
    let mut outer = None;
    for (_, raw) in lines {
        let line = shell_code(raw, &mut quoted).trim();
        if shell_function(line).is_some() {
            outer = Some(bindings.clone());
            bindings.retain(|name, _| !name.bytes().all(|c| c.is_ascii_digit()));
        } else if line == "}" {
            if let Some(saved) = outer.take() {
                bindings = saved;
            }
        }
        if line.starts_with('#') || shell_function(line).is_some() {
            continue;
        }
        for words in shell_commands(line)? {
            bind_shell_command(&words, &mut bindings);
            if matches!(words.first(), Some(&".") | Some(&"source")) {
                bindings.retain(|_, paths| !paths.iter().any(|path| {
                    b.release_roots.keys().any(|root| beneath(path, root))
                }));
            }
            let Some(position) = executable_position(&words) else { continue };
            let Some(arguments) = functions.get_mut(shell_word(words[position])) else { continue };
            let (minimum_arity, unbounded) = incomplete
                .entry(shell_word(words[position]).to_owned())
                .or_insert_with(|| (usize::MAX, BTreeSet::new()));
            *minimum_arity = (*minimum_arity).min(words.len() - position - 1);
            for (index_arg, word) in words[position + 1..].iter().enumerate() {
                let paths = declared_paths(word, &bindings);
                if paths.is_empty() {
                    unbounded.insert(index_arg + 1);
                }
                arguments.entry((index_arg + 1).to_string()).or_insert_with(BTreeSet::new).extend(paths);
            }
        }
    }
    for (name, arguments) in &functions {
        if let Some((minimum_arity, unbounded)) = incomplete.get(name) {
            for (position, paths) in arguments {
                let position = position.parse::<usize>().expect("positional binding");
                if !paths.is_empty() && (position > *minimum_arity || unbounded.contains(&position)) {
                    b.blocker(source, 0, "function path argument is not bounded at every invocation");
                }
            }
        }
    }
    Ok(functions)
}

fn scan_text(b: &mut Builder, source: &str, text: &str, shell: bool) -> Result<()> {
    let src = if source.starts_with("sv/") && source.ends_with("/run") && source.split('/').count() == 3 {
        format!("service:{}", source.split('/').nth(1).expect("service path"))
    } else {
        format!("file:{source}")
    };
    let mut bindings = b.argument_paths.get(source).cloned().unwrap_or_default();
    let host = b.host_path(source);
    if let Some(host) = &host {
        bindings.insert("0".into(), BTreeSet::from([host.clone()]));
    }
    let lines = if shell {
        shell_lines(text)
    } else {
        text.lines().enumerate().map(|(index, line)| (index + 1, std::borrow::Cow::Borrowed(line))).collect()
    };
    let functions = if shell { function_arguments(b, source, &lines)? } else { BTreeMap::new() };
    let mut outer_bindings = None;
    let mut wait_function = false;
    let mut wait_starts = false;
    let mut wait_alias = "1";
    let mut single_quote = false;
    for (line_number, raw) in &lines {
        let line_number = *line_number;
        let line = if shell && (single_quote || !raw.trim_start().starts_with("# op-dbus-consumes: ")) {
            shell_code(raw, &mut single_quote)
        } else {
            raw.as_ref()
        }.trim();
        if shell {
            if let Some(target) = line.strip_prefix("# op-dbus-consumes: ") {
                ensure!(src.starts_with("service:"), "data dependency declaration must belong to a service run script: {source}");
                ensure!(target.starts_with('/') && !target.contains(char::is_whitespace)
                    && !target.contains(['$', '`', '*', '?'])
                    && normalize_absolute(Path::new(target)).as_deref() == Some(target),
                    "invalid explicit data dependency at {source}:L{line_number}");
                ensure!(b.staged_path(target).is_some_and(|path| b.paths.contains(&path)),
                    "explicit data dependency is absent at {source}:L{line_number}");
                b.reference(&src, target, "references_path", source, line_number,
                    "explicit_published_data_dependency")?;
                continue;
            }
            if let Some(arguments) = shell_function(line).and_then(|name| functions.get(name)) {
                ensure!(outer_bindings.is_none(), "nested shell function at {source}:L{line_number}");
                let mut scoped = bindings.clone();
                scoped.retain(|name, _| !name.bytes().all(|c| c.is_ascii_digit()));
                scoped.extend(arguments.clone());
                outer_bindings = Some(std::mem::replace(&mut bindings, scoped));
                continue;
            }
            if line == "}" {
                if let Some(outer) = outer_bindings.take() {
                    bindings = outer;
                    continue;
                }
            }
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if shell && (line == "wait_dep() {" || line == "wait_dep () {") {
            wait_function = true;
            continue;
        }
        let commands = if shell { shell_commands(line)? } else { Vec::new() };
        if wait_function {
            if line == "}" {
                wait_function = false;
                continue;
            }
            if let Some((name, value)) = line.split_once('=') {
                if variable_name(name) && shell_word(value) == "$1" {
                    wait_alias = name;
                }
            }
            for words in &commands {
                if let Some((action, args)) = sv_call(&words) {
                    if let Some(arg) = args.first() {
                        let arg = shell_word(arg);
                        if arg == "$1" || arg.strip_prefix('$') == Some(wait_alias) {
                            wait_starts |= sv_starts(action);
                        } else {
                            b.blocker(source, line_number, "unresolved wait_dep function service argument");
                        }
                    }
                }
            }
            continue;
        }
        if shell {
            for words in &commands {
                bind_shell_command(words, &mut bindings);
                let assignment_words = if matches!(words.first(), Some(&"export") | Some(&"local")) {
                    &words[1..]
                } else {
                    words.as_slice()
                };
                for word in assignment_words {
                    let Some((name, value)) = word.split_once('=').filter(|(name, _)| variable_name(name)) else { break };
                    let suffix = name.rsplit('_').next().unwrap_or(name);
                    if value.contains(['$', '`']) && declared_paths(value, &bindings).is_empty() && name != "PATH"
                        && ["path", "dir", "root", "config", "bin", "manifest"]
                            .iter().any(|kind| suffix.eq_ignore_ascii_case(kind))
                    {
                        b.blocker(source, line_number, "unresolved dynamic path assignment");
                    }
                }
                if matches!(words.first(), Some(&".") | Some(&"source")) {
                    // An unread host environment can replace earlier bindings.
                    // Immutable path assignments must be pinned again afterward.
                    bindings.retain(|_, paths| !paths.iter().any(|path| {
                        b.release_roots.keys().any(|root| beneath(path, root))
                    }));
                }
                if words.first() == Some(&"wait_dep") {
                    match words.get(1).and_then(|arg| service_argument(arg)) {
                        Some(name) => b.dependency(&src, name, wait_starts, source, line_number)?,
                        None => b.blocker(source, line_number, "unresolved wait_dep service argument"),
                    }
                } else if let Some((action, args)) = sv_call(&words) {
                    if sv_starts(action) || matches!(action, "check" | "status") {
                        let mut found = false;
                        for arg in args {
                            if arg.starts_with(['>', '<']) || arg.contains('>') || arg.contains('<') {
                                break;
                            }
                            found = true;
                            match service_argument(arg) {
                                Some(name) => b.dependency(&src, name, sv_starts(action), source, line_number)?,
                                None => b.blocker(source, line_number, "unresolved sv service argument"),
                            }
                        }
                        if !found {
                            b.blocker(source, line_number, "missing sv service argument");
                        }
                    }
                }
            }
        }
        let mut paths = literal_paths(line);
        if shell {
            for words in &commands {
                // A copied bootstrap template is interpreted at its literal runtime
                // destination, not relative to the immutable source directory.
                if matches!(words.first(), Some(&"install") | Some(&"/usr/bin/install"))
                    && !words.contains(&"-d") && words.len() >= 3
                {
                    let from = declared_paths(words[words.len() - 2], &bindings);
                    let to = declared_paths(words[words.len() - 1], &bindings);
                    for host in from {
                        if let Some(staged) = b.staged_path(&host).filter(|path| {
                            b.nodes.get(&format!("file:{path}"))
                                .is_some_and(|node| node.attributes["file_type"] == "regular")
                        }) {
                            for destination in &to {
                                if let Some(parent) = Path::new(destination).parent().and_then(Path::to_str) {
                                    b.template_roots.entry(staged.clone()).or_default().insert(parent.into());
                                }
                            }
                        }
                    }
                }
                // A direct helper call supplies bounded positional path declarations.
                // No function invocation, eval, command substitution or word splitting
                // is executed to obtain these arguments.
                let program_position = executable_position(words).unwrap_or(words.len());
                if let Some(program) = words.get(program_position) {
                    if let Some(staged) = b.staged_path(shell_word(program)).filter(|path| b.paths.contains(path)) {
                        for (index, arg) in words.iter().skip(program_position + 1).enumerate() {
                            let values = declared_paths(arg, &bindings);
                            if !values.is_empty() {
                                b.argument_paths.entry(staged.clone()).or_default()
                                    .entry((index + 1).to_string()).or_default().extend(values);
                            }
                        }
                    }
                }
                for (position, raw_word) in words.iter().enumerate() {
                    // Attached, unquoted redirection operators are shell syntax,
                    // not part of the path operand (e.g. wc -c <"$file").
                    let redirected = raw_word.trim_start_matches(|c: char| c.is_ascii_digit());
                    let raw_word = if position != program_position && redirected.starts_with(['<', '>']) {
                        redirected.trim_start_matches(['<', '>'])
                    } else { raw_word };
                    let raw_word = shell_word(raw_word);
                    let path_option = |option| matches!(option,
                        "--config" | "--config-file" | "--config-dir" | "--manifest" | "--root" | "--directory" | "-c" | "." | "source");
                    let inline_config = raw_word.split_once('=').filter(|(option, _)| path_option(*option));
                    let word = shell_word(inline_config.map_or(raw_word, |(_, value)| value));
                    let command_position = position == program_position;
                    let config_argument = inline_config.is_some()
                        || (position > 0 && path_option(shell_word(words[position - 1])));
                    let declared = declared_paths(word, &bindings);
                    if word.contains(['$', '`']) && declared.is_empty()
                        && (command_position || config_argument)
                        && !matches!(word, "$@" | "$0")
                    {
                        b.blocker(source, line_number, "unresolved dynamic executable/config path");
                    }
                    if word.contains(['$', '`', '*', '?']) && !declared.is_empty()
                        && (word.contains('/') || word.contains(":-"))
                    {
                        b.conservative.insert(format!("{source}:L{line_number}: dynamic path consumes every entry under its declared bounded path alternatives."));
                    }
                    paths.extend(declared);
                }
                if words.first() == Some(&"eval") {
                    b.blocker(source, line_number, "dynamic shell evaluation");
                }
            }
        } else {
            paths.extend(config_paths(b, source, line_number, line));
        }
        for target in paths {
            if target.len() > 450 || target.contains(['$', '`'])
                || !["/etc/", "/usr/", "/opt/", "/var/", "/run/", "/dev/", "/bin/", "/sbin/"]
                    .iter().any(|prefix| target.starts_with(prefix))
            {
                continue;
            }
            let runs = shell && literal_exec_program(line) == Some(target.as_str());
            b.reference(&src, &target, if runs { "runs" } else { "references_path" },
                source, line_number, if runs { "literal_exec_candidate" } else { "static_path_reference" })?;
        }
    }
    if outer_bindings.is_some() {
        b.blocker(source, text.lines().count(), "unterminated supported shell function");
    }
    if wait_function {
        b.blocker(source, text.lines().count(), "unterminated supported wait_dep function");
    }
    Ok(())
}

fn collect_consumers(b: &mut Builder) -> Result<()> {
    let mut adjacency: BTreeMap<&str, Vec<&GraphEdge>> = BTreeMap::new();
    for edge in &b.edges {
        adjacency.entry(&edge.src).or_default().push(edge);
    }
    let mut consumers = BTreeMap::<String, BTreeSet<String>>::new();
    let mut service_properties = Vec::new();
    let mut blockers = BTreeSet::new();
    for service in b.nodes.values().filter(|node| {
        node.kind == "runit_service" && node.attributes.get("release_owned") == Some(&json!(true))
    }) {
        let mut seen = BTreeSet::new();
        let mut pending = vec![service.id.as_str()];
        let mut deps = BTreeSet::new();
        let mut starts = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(errors) = b.blockers.get(id) {
                for error in errors {
                    blockers.insert(format!("{}: {error}", service.label));
                }
            }
            if let Some(path) = id.strip_prefix("file:") {
                consumers.entry(path.into()).or_default().insert(service.label.clone());
                // Ancestor metadata affects lookup; do not expand siblings from here.
                let mut parent = Path::new(path).parent();
                while let Some(path) = parent.filter(|p| !p.as_os_str().is_empty()) {
                    consumers.entry(path.to_str().context("non-UTF8 ancestor")?.into())
                        .or_default().insert(service.label.clone());
                    parent = path.parent();
                }
            }
            for edge in adjacency.get(id).into_iter().flatten() {
                match edge.relation.as_str() {
                    "depends_on" | "starts" => {
                        let name = edge.dst.strip_prefix("service:").context("invalid service reference")?;
                        if name != service.label {
                            deps.insert(name.to_owned());
                            if edge.relation == "starts" {
                                starts.insert(name.to_owned());
                            }
                        }
                    }
                    "configured_by" | "contains" | "runs" | "references_path" | "provenance_document" => {
                        pending.push(&edge.dst);
                    }
                    _ => {}
                }
            }
        }
        service_properties.push((service.id.clone(), deps, starts));
    }
    ensure!(blockers.is_empty(), "restart inventory is incomplete:\n{}", blockers.into_iter().collect::<Vec<_>>().join("\n"));
    drop(adjacency);
    for (path, names) in consumers {
        if let Some(node) = b.nodes.get_mut(&format!("file:{path}")) {
            node.attributes.insert("consumers".into(), json!(names));
        }
    }
    for (id, deps, starts) in service_properties {
        let node = b.nodes.get_mut(&id).expect("service node");
        node.attributes.insert("deps".into(), json!(deps));
        node.attributes.insert("starts".into(), json!(starts));
    }
    Ok(())
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
    b.release_targets()?;
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
    let mut limitations = vec![
        "Prepared paths and declarations are not deployment, running processes, mounted subvolumes or live backend state.".into(),
        "Shell analysis unions literal paths, simple path assignments and supported wait_dep/sv declarations across branches; it never executes shell or reads host environment overrides.".into(),
        "Quoted words and backslash continuations are preserved; nested substitution operands are dependencies, not executed values. Path-producing substitutions are limited to lexical readlink -f/dirname/CDPATH+pwd and mktemp templates; other dynamic executable/config/include paths block completeness.".into(),
        "Standalone function path arguments must be bounded at every call; non-path arguments do not become dependencies. unix:path= defaults and exact prefix removal declare external socket paths, not runtime address validation.".into(),
        "Captured-source JSON with repository/commit/source_path/files provenance structure is hashed metadata, not runtime configuration; unrecognized or mixed JSON retains configuration parsing.".into(),
        "Secrets and credential/environment files are never read; source line content is never retained.".into(),
        "Archives are inventoried as artifacts; their internal container filesystem is not unpacked.".into(),
        "The graph output subtree and MANIFEST file digest are excluded to prevent self-reference.".into(),
    ];
    let manifests = [
        (format!("{SERVICE_MANIFEST_DIR}/enabled-services"), "enabled"),
        (format!("{SERVICE_MANIFEST_DIR}/retired-services"), "retired"),
    ];
    let mut memberships: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (path, state) in manifests {
        ensure!(b.paths.contains(&path), "missing required service declarations: {path}");
        for (i, line) in read_text(&root, &path)?.lines().enumerate() {
            let name = line.split('#').next().unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            ensure!(valid_service(name), "invalid staged service name");
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
                    json!({"authority":"prepared_declaration","release_owned":false,"definition_staged":false,"deps":[],"starts":[]}),
                )?;
            }
            b.edges.push(edge(
                id,
                &format!("declared_{state}"),
                format!("file:{path}"),
                &path,
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
        ensure!(valid_service(name), "invalid staged service name");
        let text = read_text(&root, path)?;
        let declared = memberships.get(name).context("staged service has no lifecycle declaration")?;
        ensure!(
            declared.contains("enabled") && !declared.contains("retired")
                && text.lines().any(|line| line.trim() == "# op-dbus-release: application"),
            "staged service {name} is not an enabled release-owned application"
        );
        let service = format!("service:{name}");
        let node = b.nodes.get_mut(&service).expect("declared service");
        node.attributes.insert("release_owned".into(), json!(true));
        node.attributes.insert("definition_staged".into(), json!(true));
        b.edges.push(edge(&service, "configured_by", format!("file:sv/{name}"),
            path, "service_directory", json!({})));
        scan_text(&mut b, path, &text, true)?;
    }
    let paths: Vec<_> = b.paths.iter().cloned().collect();
    for path in paths {
        if runs.contains(&path) || sensitive(&path) {
            continue;
        }
        let node = &b.nodes[&format!("file:{path}")];
        if node.attributes["file_type"] == "symlink" {
            let raw = node.attributes["target"].as_str().context("invalid symlink declaration")?;
            let host = b.host_path(&path);
            let target = if raw.starts_with('/') {
                Some(PathBuf::from(raw))
            } else {
                host.as_deref().and_then(|host| Path::new(host).parent()).map(|parent| parent.join(raw))
            };
            if let Some(target) = target.and_then(|path| normalize_absolute(&path)) {
                b.reference(&format!("file:{path}"), &target, "references_path",
                    &path, 0, "lexical_symlink_declaration_not_followed")?;
            }
            continue;
        }
        if node.attributes["file_type"] != "regular" {
            continue;
        }
        // Vendor runtimes are consumed as complete immutable bundles. Analyze
        // our launchers, not arbitrary bundled third-party shell programs.
        let script_candidate = path.starts_with("sv/") || path.starts_with("bin/")
            || path.starts_with("libexec/3tched/")
            || path.starts_with("usr/share/op-dbus/notebook-sync/scripts/");
        let config = category(&path) == "configuration" && !path.starts_with("opt/")
            && Path::new(&path).extension().and_then(|extension| extension.to_str())
                .is_some_and(|extension| matches!(extension, "conf" | "toml" | "hocon" | "json" | "ini" | "cf" | "yaml" | "yml"));
        if !script_candidate && !config {
            continue;
        }
        let shell = if script_candidate {
            let mut file = OpenOptions::new().read(true)
                .custom_flags(nix::libc::O_NOFOLLOW).open(root.join(&path))?;
            let mut prefix = [0u8; 128];
            let count = file.read(&mut prefix)?;
            let first = prefix[..count].split(|byte| *byte == b'\n').next().unwrap_or(&[]);
            first.starts_with(b"#!") && (first.ends_with(b"/sh") || first.ends_with(b"/bash") || first.ends_with(b" sh") || first.ends_with(b" bash"))
        } else {
            false
        };
        if !shell && !config {
            continue;
        }
        if fs::symlink_metadata(root.join(&path))?.len() > MAX_TEXT {
            b.blocker(&path, 0, "referenced text exceeds parsing bound");
            continue;
        }
        match read_text(&root, &path) {
            Ok(text) if config && !shell && path.ends_with(".json") && source_provenance(&text) => {
                b.nodes.get_mut(&format!("file:{path}")).expect("prepared file")
                    .attributes.insert("category".into(), json!("provenance"));
            }
            Ok(text) => scan_text(&mut b, &path, &text, shell)?,
            Err(_) => b.blocker(&path, 0, "referenced configuration is not readable bounded UTF-8"),
        }
    }
    // Provenance documents are linked by category rather than copying arbitrary
    // JSON contents (which could contain future private fields).
    for (document, prefix) in [
        (
            "usr/share/op-dbus/threetched-manifests/tched-router.json",
            "usr/bin/threetched",
        ),
        (
            "usr/share/op-dbus/ui-manifests/3tched-ui.json",
            "usr/local/share/op-dbus/dashboard/",
        ),
        (
            "usr/share/op-dbus/plugin-blobs/.manifest.json",
            "usr/share/op-dbus/plugin-blobs/",
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
    let catalog_path = "usr/share/op-dbus/plugin-blobs/.manifest.json";
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
            let blob = format!("{CATALOG_DIR}/{name}.{}.blob", &hash[..16]);
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
    collect_consumers(&mut b)?;
    limitations.extend(b.conservative);
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
