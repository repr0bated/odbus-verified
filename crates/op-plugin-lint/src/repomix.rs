//! Introspect a Repomix XML pack into element paths from **schema-convertible**
//! structured data:
//!
//! | Kind | Extensions / sniff | Path prefix |
//! |---|---|---|
//! | Rust | `.rs` | `struct.` / `enum.` |
//! | TOML | `.toml` | `toml.` |
//! | YAML | `.yml` / `.yaml` | `yaml.` (or `openapi.` if sniffed) |
//! | JSON | `.json` | `json.` / `jsonschema.` / `openapi.` / `avro.` |
//! | SQL | `.sql` | `sql.` (CREATE TABLE/TYPE/VIEW) |
//! | Protobuf | `.proto` | `proto.` |
//! | GraphQL | `.graphql` / `.gql` | `graphql.` |
//! | Prisma | `.prisma` | `prisma.` |
//! | Avro | `.avsc` | `avro.` |
//! | Thrift | `.thrift` | `thrift.` |
//! | Cap'n Proto | `.capnp` | `capnp.` |
//! | FlatBuffers | `.fbs` | `fbs.` |
//! | XML Schema | `.xsd` | `xsd.` |
//! | CSV | `.csv` | `csv.` (header columns) |
//! | Python | `.py` (classes / pydantic fields) | `py.` |

use crate::gadget::introspect_json_paths;
use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use syn::{Fields, Item};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredKind {
    Rust,
    Toml,
    Yaml,
    Json,
    JsonSchema,
    OpenApi,
    Sql,
    Proto,
    Graphql,
    Prisma,
    Avro,
    Thrift,
    Capnp,
    Flatbuffers,
    Xsd,
    Csv,
    Python,
}

impl StructuredKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
            Self::Json => "json",
            Self::JsonSchema => "jsonschema",
            Self::OpenApi => "openapi",
            Self::Sql => "sql",
            Self::Proto => "proto",
            Self::Graphql => "graphql",
            Self::Prisma => "prisma",
            Self::Avro => "avro",
            Self::Thrift => "thrift",
            Self::Capnp => "capnp",
            Self::Flatbuffers => "flatbuffers",
            Self::Xsd => "xsd",
            Self::Csv => "csv",
            Self::Python => "python",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StructuredFile {
    pub path: String,
    pub kind: StructuredKind,
    pub element_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepomixSurface {
    pub source: String,
    pub kind: &'static str,
    pub files_seen: usize,
    /// Structured file counts by kind label.
    pub files_by_kind: BTreeMap<String, usize>,
    /// Element-path counts by kind label.
    pub by_kind: BTreeMap<String, usize>,
    pub structured_files: Vec<StructuredFile>,
    pub element_paths: Vec<String>,
}

impl RepomixSurface {
    pub fn file_count(&self, kind: &str) -> usize {
        self.files_by_kind.get(kind).copied().unwrap_or(0)
    }
}

pub fn looks_like_repomix(path: &Path, text: &str) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name.contains("repomix") && (name.ends_with(".xml") || name.ends_with(".md")) {
        return true;
    }
    text.contains("<file_summary>")
        && text.contains("<file path=")
        && (text.contains("Repomix") || text.contains("repomix"))
}

pub fn introspect_repomix(path: &Path, text: &str) -> Result<RepomixSurface> {
    if !looks_like_repomix(path, text) && !text.contains("<file path=") {
        bail!("{} does not look like a Repomix pack", path.display());
    }

    let mut paths = BTreeSet::new();
    let mut structured_files = Vec::new();
    let mut files_seen = 0usize;
    let mut files_by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();

    for (file_path, body) in iter_repomix_files(text) {
        files_seen += 1;
        if skip_file(&file_path) {
            continue;
        }

        let Some((kind, extracted)) = extract_structured(&file_path, &body) else {
            continue;
        };

        let label = kind.label().to_string();
        *files_by_kind.entry(label.clone()).or_default() += 1;
        *by_kind.entry(label).or_default() += extracted.len();
        structured_files.push(StructuredFile {
            path: file_path,
            kind,
            element_count: extracted.len(),
        });
        paths.extend(extracted);
    }

    if files_seen == 0 {
        bail!("no <file path=…> entries found in {}", path.display());
    }

    structured_files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(RepomixSurface {
        source: path.display().to_string(),
        kind: "repomix",
        files_seen,
        files_by_kind,
        by_kind,
        structured_files,
        element_paths: paths.into_iter().collect(),
    })
}

pub fn surface_to_coverage_json(surface: &RepomixSurface) -> Result<String> {
    let v = serde_json::json!({
        "binary": format!("repomix:{}", surface.source),
        "version": "repomix",
        "kind": surface.kind,
        "files_seen": surface.files_seen,
        "files_by_kind": surface.files_by_kind,
        "by_kind": surface.by_kind,
        "structured_files": surface.structured_files,
        "nodes": [],
        "element_paths": surface.element_paths,
    });
    Ok(serde_json::to_string_pretty(&v)?)
}

fn extract_structured(file_path: &str, body: &str) -> Option<(StructuredKind, Vec<String>)> {
    let lower = file_path.to_ascii_lowercase();

    if lower.ends_with(".rs") {
        let crate_hint = crate_hint_from_path(file_path);
        return Some((
            StructuredKind::Rust,
            extract_rust_element_paths(file_path, body, crate_hint.as_deref()),
        ));
    }
    if lower.ends_with(".py") {
        // Skip unit tests — schema surface is production modules.
        if lower.ends_with("_test.py") || lower.ends_with("/test_") || lower.contains("/tests/") {
            return None;
        }
        return Some((
            StructuredKind::Python,
            extract_python_element_paths(file_path, body),
        ));
    }
    if lower.ends_with(".toml") {
        return Some((StructuredKind::Toml, extract_toml_paths(file_path, body)));
    }
    if lower.ends_with(".yml") || lower.ends_with(".yaml") {
        return Some(extract_yaml_or_openapi(file_path, body));
    }
    if lower.ends_with(".avsc") {
        return Some((
            StructuredKind::Avro,
            extract_jsonish(file_path, body, "avro"),
        ));
    }
    if lower.ends_with(".json") {
        return Some(extract_json_family(file_path, body));
    }
    if lower.ends_with(".sql") {
        return Some((StructuredKind::Sql, extract_sql_paths(file_path, body)));
    }
    if lower.ends_with(".proto") {
        return Some((StructuredKind::Proto, extract_proto_paths(file_path, body)));
    }
    if lower.ends_with(".graphql") || lower.ends_with(".gql") {
        return Some((
            StructuredKind::Graphql,
            extract_graphql_paths(file_path, body),
        ));
    }
    if lower.ends_with(".prisma") {
        return Some((
            StructuredKind::Prisma,
            extract_prisma_paths(file_path, body),
        ));
    }
    if lower.ends_with(".thrift") {
        return Some((
            StructuredKind::Thrift,
            extract_thrift_paths(file_path, body),
        ));
    }
    if lower.ends_with(".capnp") {
        return Some((StructuredKind::Capnp, extract_capnp_paths(file_path, body)));
    }
    if lower.ends_with(".fbs") {
        return Some((
            StructuredKind::Flatbuffers,
            extract_fbs_paths(file_path, body),
        ));
    }
    if lower.ends_with(".xsd") {
        return Some((StructuredKind::Xsd, extract_xsd_paths(file_path, body)));
    }
    if lower.ends_with(".csv") {
        return Some((StructuredKind::Csv, extract_csv_paths(file_path, body)));
    }
    None
}

fn skip_file(file_path: &str) -> bool {
    let lower = file_path.to_ascii_lowercase();
    let name = Path::new(file_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name == "cargo.lock"
        || name == "package-lock.json"
        || name == "pnpm-lock.yaml"
        || name == "yarn.lock"
        || name.ends_with(".lock")
        || lower.contains("/target/")
        || lower.contains("/node_modules/")
        || lower.contains("/.git/")
}

fn file_key(file_path: &str) -> String {
    let lower = file_path.to_ascii_lowercase();
    let strip = [
        ".graphql",
        ".flatbuffers", // not used
        ".yaml",
        ".yml",
        ".toml",
        ".json",
        ".avsc",
        ".proto",
        ".prisma",
        ".thrift",
        ".capnp",
        ".fbs",
        ".xsd",
        ".csv",
        ".sql",
        ".gql",
        ".py",
        ".rs",
    ];
    let mut stem = file_path;
    for ext in strip {
        if lower.ends_with(ext) {
            stem = &file_path[..file_path.len() - ext.len()];
            break;
        }
    }
    sanitize_path(stem)
        .split('.')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}

/// Extract `class Name` and annotated attributes (`field: Type`) from Python.
fn extract_python_element_paths(file_path: &str, body: &str) -> Vec<String> {
    let base = format!("py.{}", file_key(file_path));
    let mut out = BTreeSet::new();
    out.insert(base.clone());

    // class Foo(Bar): / class Foo:
    let class_re = re(r"(?m)^class\s+([A-Za-z_][\w]*)\s*[:(]");
    // skip test noise filenames later via path; still extract
    let field_re = re(r"(?m)^(?:\s{4}|\t)([A-Za-z_][\w]*)\s*:\s*([^\n=#]+)");
    let enum_member = re(r"(?m)^(?:\s{4}|\t)([A-Z][A-Z0-9_]*)\s*=");

    // Split roughly by class blocks
    let class_iter: Vec<(usize, String)> = class_re
        .captures_iter(body)
        .filter_map(|c| {
            let name = c.get(1)?.as_str().to_string();
            let start = c.get(0)?.start();
            Some((start, name))
        })
        .collect();

    if class_iter.is_empty() {
        return out.into_iter().collect();
    }

    for (i, (start, name)) in class_iter.iter().enumerate() {
        let end = class_iter.get(i + 1).map(|(s, _)| *s).unwrap_or(body.len());
        let block = &body[*start..end];
        let cpath = format!("{base}.class.{name}");
        out.insert(cpath.clone());

        let is_enum = block.contains("Enum") || block.contains("enum.Enum");
        if is_enum {
            for m in enum_member.captures_iter(block) {
                if let Some(v) = m.get(1) {
                    out.insert(format!("{cpath}.{}", v.as_str()));
                }
            }
        } else {
            for f in field_re.captures_iter(block) {
                let Some(fname) = f.get(1) else { continue };
                let name = fname.as_str();
                if name == "self" || name.starts_with("__") {
                    continue;
                }
                // Control-flow / false positives from `else:` etc.
                if matches!(
                    name,
                    "if" | "elif"
                        | "else"
                        | "for"
                        | "while"
                        | "try"
                        | "except"
                        | "finally"
                        | "with"
                        | "return"
                        | "yield"
                        | "assert"
                        | "class"
                        | "def"
                        | "async"
                        | "await"
                        | "pass"
                        | "raise"
                        | "from"
                        | "import"
                        | "global"
                        | "nonlocal"
                        | "lambda"
                        | "match"
                        | "case"
                ) {
                    continue;
                }
                out.insert(format!("{cpath}.{name}"));
            }
        }
    }
    out.into_iter().collect()
}

fn extract_toml_paths(file_path: &str, body: &str) -> Vec<String> {
    let Ok(value) = body.parse::<toml::Value>() else {
        return vec![format!("toml.{}.<parse_error>", file_key(file_path))];
    };
    let Ok(json) = serde_json::to_value(value) else {
        return Vec::new();
    };
    prefix_paths("toml", file_path, &json)
}

fn extract_yaml_or_openapi(file_path: &str, body: &str) -> (StructuredKind, Vec<String>) {
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(body) else {
        return (
            StructuredKind::Yaml,
            vec![format!("yaml.{}.<parse_error>", file_key(file_path))],
        );
    };
    let Ok(json) = serde_json::to_value(value) else {
        return (StructuredKind::Yaml, Vec::new());
    };
    if is_openapi(&json) {
        (
            StructuredKind::OpenApi,
            prefix_paths("openapi", file_path, &json),
        )
    } else if json.is_null() {
        (
            StructuredKind::Yaml,
            vec![format!("yaml.{}", file_key(file_path))],
        )
    } else {
        (StructuredKind::Yaml, prefix_paths("yaml", file_path, &json))
    }
}

fn extract_json_family(file_path: &str, body: &str) -> (StructuredKind, Vec<String>) {
    let Ok(json) = serde_json::from_str::<JsonValue>(body) else {
        return (
            StructuredKind::Json,
            vec![format!("json.{}.<parse_error>", file_key(file_path))],
        );
    };
    if is_openapi(&json) {
        (
            StructuredKind::OpenApi,
            prefix_paths("openapi", file_path, &json),
        )
    } else if is_json_schema(&json) {
        (
            StructuredKind::JsonSchema,
            prefix_paths("jsonschema", file_path, &json),
        )
    } else if is_avro_schema(&json) {
        (StructuredKind::Avro, prefix_paths("avro", file_path, &json))
    } else {
        (StructuredKind::Json, prefix_paths("json", file_path, &json))
    }
}

fn extract_jsonish(file_path: &str, body: &str, fmt: &str) -> Vec<String> {
    let Ok(json) = serde_json::from_str::<JsonValue>(body) else {
        return vec![format!("{fmt}.{}.<parse_error>", file_key(file_path))];
    };
    prefix_paths(fmt, file_path, &json)
}

fn is_openapi(json: &JsonValue) -> bool {
    json.get("openapi").is_some() || json.get("swagger").is_some()
}

fn is_json_schema(json: &JsonValue) -> bool {
    json.get("$schema").is_some()
        && (json.get("properties").is_some()
            || json.get("type").is_some()
            || json.get("$defs").is_some()
            || json.get("definitions").is_some())
}

fn is_avro_schema(json: &JsonValue) -> bool {
    json.get("type").and_then(|t| t.as_str()) == Some("record") && json.get("fields").is_some()
}

fn prefix_paths(fmt: &str, file_path: &str, json: &JsonValue) -> Vec<String> {
    let base = format!("{fmt}.{}", file_key(file_path));
    let mut out = BTreeSet::new();
    out.insert(base.clone());
    for p in introspect_json_paths(json) {
        out.insert(format!("{base}.{p}"));
    }
    out.into_iter().collect()
}

// ── SQL ──────────────────────────────────────────────────────────────────────

fn extract_sql_paths(file_path: &str, body: &str) -> Vec<String> {
    let base = format!("sql.{}", file_key(file_path));
    let mut out = BTreeSet::new();
    out.insert(base.clone());

    let create_table =
        re(r"(?is)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(?:\w+\.)?(\w+)\s*\((.*?)\)");
    let create_type = re(r"(?is)CREATE\s+TYPE\s+(?:\w+\.)?(\w+)");
    let create_view = re(r"(?is)CREATE\s+(?:OR\s+REPLACE\s+)?VIEW\s+(?:\w+\.)?(\w+)");
    // Type token stops at whitespace/comma so we don't swallow following columns.
    let col = re(r"(?m)^\s*([A-Za-z_][\w]*)\s+([A-Za-z][\w()]*)");

    for cap in create_table.captures_iter(body) {
        let table = cap.get(1).map(|m| m.as_str()).unwrap_or("table");
        let cols = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let tpath = format!("{base}.table.{table}");
        out.insert(tpath.clone());
        for c in col.captures_iter(cols) {
            let name = c.get(1).map(|m| m.as_str()).unwrap_or("");
            if name.is_empty() || is_sql_keyword(name) {
                continue;
            }
            out.insert(format!("{tpath}.{name}"));
        }
    }
    for cap in create_type.captures_iter(body) {
        if let Some(name) = cap.get(1) {
            out.insert(format!("{base}.type.{}", name.as_str()));
        }
    }
    for cap in create_view.captures_iter(body) {
        if let Some(name) = cap.get(1) {
            out.insert(format!("{base}.view.{}", name.as_str()));
        }
    }
    out.into_iter().collect()
}

fn is_sql_keyword(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "PRIMARY"
            | "FOREIGN"
            | "UNIQUE"
            | "CHECK"
            | "CONSTRAINT"
            | "INDEX"
            | "KEY"
            | "REFERENCES"
            | "ON"
            | "NOT"
            | "NULL"
            | "DEFAULT"
    )
}

// ── Protobuf ─────────────────────────────────────────────────────────────────

fn extract_proto_paths(file_path: &str, body: &str) -> Vec<String> {
    let base = format!("proto.{}", file_key(file_path));
    let mut out = BTreeSet::new();
    out.insert(base.clone());

    let msg = re(r"(?m)^\s*(?:export\s+)?message\s+(\w+)\s*\{");
    let enm = re(r"(?m)^\s*(?:export\s+)?enum\s+(\w+)\s*\{");
    let svc = re(r"(?m)^\s*service\s+(\w+)\s*\{");
    let field = re(r"(?m)^\s*(?:repeated\s+|optional\s+|required\s+)?([\w\.]+)\s+(\w+)\s*=");
    let rpc = re(r"(?m)^\s*rpc\s+(\w+)\s*\(");

    // Block-scoped field extraction: crude split on message/enum bodies.
    let block = re(r"(?s)(message|enum|service)\s+(\w+)\s*\{(.*?)\}");
    for cap in block.captures_iter(body) {
        let kind = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let name = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let body_inner = cap.get(3).map(|m| m.as_str()).unwrap_or("");
        let tpath = format!("{base}.{kind}.{name}");
        out.insert(tpath.clone());
        match kind {
            "message" => {
                for f in field.captures_iter(body_inner) {
                    if let Some(fname) = f.get(2) {
                        out.insert(format!("{tpath}.{}", fname.as_str()));
                    }
                }
            }
            "enum" => {
                let variant = re(r"(?m)^\s*([A-Z_][A-Z0-9_]*)\s*=");
                for v in variant.captures_iter(body_inner) {
                    if let Some(vname) = v.get(1) {
                        out.insert(format!("{tpath}.{}", vname.as_str()));
                    }
                }
            }
            "service" => {
                for r in rpc.captures_iter(body_inner) {
                    if let Some(rname) = r.get(1) {
                        out.insert(format!("{tpath}.rpc.{}", rname.as_str()));
                    }
                }
            }
            _ => {}
        }
    }

    // Top-level declarations if block regex missed nested-less files.
    for cap in msg.captures_iter(body) {
        if let Some(n) = cap.get(1) {
            out.insert(format!("{base}.message.{}", n.as_str()));
        }
    }
    for cap in enm.captures_iter(body) {
        if let Some(n) = cap.get(1) {
            out.insert(format!("{base}.enum.{}", n.as_str()));
        }
    }
    for cap in svc.captures_iter(body) {
        if let Some(n) = cap.get(1) {
            out.insert(format!("{base}.service.{}", n.as_str()));
        }
    }
    out.into_iter().collect()
}

// ── GraphQL ──────────────────────────────────────────────────────────────────

fn extract_graphql_paths(file_path: &str, body: &str) -> Vec<String> {
    let base = format!("graphql.{}", file_key(file_path));
    let mut out = BTreeSet::new();
    out.insert(base.clone());

    let block = re(
        r"(?s)\b(type|input|enum|interface|union|scalar)\s+(\w+)(?:\s+implements\s+[\w&\s]+)?\s*\{(.*?)\}",
    );
    let field = re(r"(?m)^\s*(\w+)(\s*\([^)]*\))?\s*:");
    let scalar = re(r"(?m)^\s*scalar\s+(\w+)");
    let union_ = re(r"(?m)^\s*union\s+(\w+)\s*=");

    for cap in block.captures_iter(body) {
        let kind = cap.get(1).map(|m| m.as_str()).unwrap_or("type");
        let name = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let inner = cap.get(3).map(|m| m.as_str()).unwrap_or("");
        let tpath = format!("{base}.{kind}.{name}");
        out.insert(tpath.clone());
        if kind == "enum" {
            for line in inner.lines() {
                let v = line.trim();
                if v.is_empty() || v.starts_with('#') {
                    continue;
                }
                let vname = v.split_whitespace().next().unwrap_or("");
                if !vname.is_empty() {
                    out.insert(format!("{tpath}.{vname}"));
                }
            }
        } else {
            for f in field.captures_iter(inner) {
                if let Some(fname) = f.get(1) {
                    out.insert(format!("{tpath}.{}", fname.as_str()));
                }
            }
        }
    }
    for cap in scalar.captures_iter(body) {
        if let Some(n) = cap.get(1) {
            out.insert(format!("{base}.scalar.{}", n.as_str()));
        }
    }
    for cap in union_.captures_iter(body) {
        if let Some(n) = cap.get(1) {
            out.insert(format!("{base}.union.{}", n.as_str()));
        }
    }
    out.into_iter().collect()
}

// ── Prisma ───────────────────────────────────────────────────────────────────

fn extract_prisma_paths(file_path: &str, body: &str) -> Vec<String> {
    let base = format!("prisma.{}", file_key(file_path));
    let mut out = BTreeSet::new();
    out.insert(base.clone());
    let block = re(r"(?s)\b(model|enum|type|view)\s+(\w+)\s*\{(.*?)\}");
    let field = re(r"(?m)^\s*(\w+)\s+");
    for cap in block.captures_iter(body) {
        let kind = cap.get(1).map(|m| m.as_str()).unwrap_or("model");
        let name = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let inner = cap.get(3).map(|m| m.as_str()).unwrap_or("");
        let tpath = format!("{base}.{kind}.{name}");
        out.insert(tpath.clone());
        for f in field.captures_iter(inner) {
            if let Some(fname) = f.get(1) {
                let n = fname.as_str();
                if n.starts_with("@@") || n == "model" {
                    continue;
                }
                out.insert(format!("{tpath}.{n}"));
            }
        }
    }
    out.into_iter().collect()
}

// ── Thrift / Cap'n / FlatBuffers / XSD / CSV ─────────────────────────────────

fn extract_thrift_paths(file_path: &str, body: &str) -> Vec<String> {
    extract_keyword_blocks(
        file_path,
        body,
        "thrift",
        &["struct", "enum", "service", "union"],
    )
}

fn extract_capnp_paths(file_path: &str, body: &str) -> Vec<String> {
    extract_keyword_blocks(file_path, body, "capnp", &["struct", "enum", "interface"])
}

fn extract_fbs_paths(file_path: &str, body: &str) -> Vec<String> {
    extract_keyword_blocks(
        file_path,
        body,
        "fbs",
        &["table", "struct", "enum", "union"],
    )
}

fn extract_keyword_blocks(
    file_path: &str,
    body: &str,
    fmt: &str,
    keywords: &[&str],
) -> Vec<String> {
    let base = format!("{fmt}.{}", file_key(file_path));
    let mut out = BTreeSet::new();
    out.insert(base.clone());
    let kw = keywords.join("|");
    let block = Regex::new(&format!(r"(?s)\b({kw})\s+(\w+)\s*\{{(.*?)\}}")).expect("regex");
    let field = re(r"(?m)^\s*(?:\d+\s*:\s*)?(?:optional\s+|required\s+)?(?:[\w\.:<>]+)\s+(\w+)");
    for cap in block.captures_iter(body) {
        let kind = cap.get(1).map(|m| m.as_str()).unwrap_or("struct");
        let name = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let inner = cap.get(3).map(|m| m.as_str()).unwrap_or("");
        let tpath = format!("{base}.{kind}.{name}");
        out.insert(tpath.clone());
        for f in field.captures_iter(inner) {
            if let Some(fname) = f.get(1) {
                out.insert(format!("{tpath}.{}", fname.as_str()));
            }
        }
    }
    out.into_iter().collect()
}

fn extract_xsd_paths(file_path: &str, body: &str) -> Vec<String> {
    let base = format!("xsd.{}", file_key(file_path));
    let mut out = BTreeSet::new();
    out.insert(base.clone());
    let names = re(
        r#"(?i)<(?:xs:|xsd:)?(element|complexType|simpleType|attribute)\s+[^>]*name\s*=\s*"([^"]+)""#,
    );
    for cap in names.captures_iter(body) {
        let kind = cap.get(1).map(|m| m.as_str()).unwrap_or("element");
        let name = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        out.insert(format!("{base}.{kind}.{name}"));
    }
    out.into_iter().collect()
}

fn extract_csv_paths(file_path: &str, body: &str) -> Vec<String> {
    let base = format!("csv.{}", file_key(file_path));
    let mut out = BTreeSet::new();
    out.insert(base.clone());
    let Some(header) = body.lines().next() else {
        return out.into_iter().collect();
    };
    for col in header.split(',') {
        let name = col.trim().trim_matches('"').trim_matches('\'');
        if !name.is_empty() {
            out.insert(format!("{base}.{name}"));
        }
    }
    out.into_iter().collect()
}

fn re(pat: &str) -> Regex {
    Regex::new(pat).expect("valid regex")
}

// Rebind local helpers that called re() — replace with re() below via search.

fn crate_hint_from_path(file_path: &str) -> Option<String> {
    let parts: Vec<&str> = file_path.split('/').collect();
    if let Some(i) = parts.iter().position(|p| *p == "crates") {
        if let Some(name) = parts.get(i + 1) {
            return Some(name.replace('-', "_"));
        }
    }
    if parts.first() == Some(&"src") {
        return Some("zeroclaw".to_string());
    }
    None
}

fn iter_repomix_files(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    const OPEN: &str = "<file path=\"";
    const CLOSE: &str = "</file>";
    while let Some(start) = rest.find(OPEN) {
        let after_open = &rest[start + OPEN.len()..];
        let Some(quote) = after_open.find('"') else {
            break;
        };
        let path = after_open[..quote].to_string();
        let after_path = &after_open[quote + 1..];
        let body_start = match after_path.strip_prefix('>') {
            Some(s) => s.strip_prefix('\n').unwrap_or(s),
            None => {
                rest = after_path;
                continue;
            }
        };
        let Some(end) = body_start.find(CLOSE) else {
            break;
        };
        let body = body_start[..end].to_string();
        out.push((path, body));
        rest = &body_start[end + CLOSE.len()..];
    }
    out
}

fn extract_rust_element_paths(file_path: &str, src: &str, crate_hint: Option<&str>) -> Vec<String> {
    let Ok(file) = syn::parse_file(src) else {
        return vec![format!("file.{}", sanitize_path(file_path))];
    };

    let prefix = crate_hint.unwrap_or("rs");
    let mut out = Vec::new();

    for item in file.items {
        match item {
            Item::Struct(s) => {
                let name = s.ident.to_string();
                out.push(format!("struct.{prefix}.{name}"));
                push_fields(&mut out, prefix, &name, &s.fields);
            }
            Item::Enum(e) => {
                let name = e.ident.to_string();
                out.push(format!("enum.{prefix}.{name}"));
                for v in e.variants {
                    let vname = v.ident.to_string();
                    out.push(format!("enum.{prefix}.{name}.{vname}"));
                    push_fields(&mut out, prefix, &format!("{name}.{vname}"), &v.fields);
                }
            }
            _ => {}
        }
    }

    if out.is_empty() {
        out.push(format!("file.{}", sanitize_path(file_path)));
    }
    out
}

fn push_fields(out: &mut Vec<String>, prefix: &str, type_name: &str, fields: &Fields) {
    match fields {
        Fields::Named(n) => {
            for f in &n.named {
                if let Some(ident) = &f.ident {
                    out.push(format!("struct.{prefix}.{type_name}.{}", ident));
                }
            }
        }
        Fields::Unnamed(u) => {
            for (i, _) in u.unnamed.iter().enumerate() {
                out.push(format!("struct.{prefix}.{type_name}.{i}"));
            }
        }
        Fields::Unit => {}
    }
}

fn sanitize_path(p: &str) -> String {
    p.replace(['/', '\\'], ".")
}

pub fn read_and_introspect(path: &Path) -> Result<RepomixSurface> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read repomix {}", path.display()))?;
    introspect_repomix(path, &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn extracts_struct_fields_from_repomix_xml() {
        let xml = r#"
<file_summary>Repomix pack</file_summary>
<file path="crates/zeroclaw-config/src/schema/v1.rs">
pub struct V1Config {
    pub default_model: Option<String>,
    pub default_provider: Option<String>,
}
</file>
"#;
        let surface = introspect_repomix(Path::new("repomix-output.xml"), xml).unwrap();
        assert_eq!(surface.file_count("rust"), 1);
        assert!(surface
            .element_paths
            .iter()
            .any(|p| p == "struct.zeroclaw_config.V1Config.default_model"));
    }

    #[test]
    fn identifies_toml_yaml_sql_proto_graphql() {
        let xml = r#"
<file_summary>Repomix pack</file_summary>
<file path="config/default.toml">
default_model = "gpt-4"
[providers.openrouter]
api_key = "secret"
</file>
<file path=".github/workflows/ci.yml">
name: ci
jobs:
  test:
    runs-on: ubuntu-latest
</file>
<file path="db/schema.sql">
CREATE TABLE users (
  id BIGINT PRIMARY KEY,
  email TEXT NOT NULL,
  created_at TIMESTAMP
);
CREATE TYPE mood AS ENUM ('happy');
</file>
<file path="api/v1.proto">
syntax = "proto3";
message User {
  string id = 1;
  string email = 2;
}
service UserService {
  rpc GetUser (User) returns (User);
}
</file>
<file path="schema.graphql">
type User {
  id: ID!
  email: String!
}
input CreateUserInput {
  email: String!
}
enum Role {
  ADMIN
  USER
}
</file>
<file path="openapi.yaml">
openapi: 3.0.0
info:
  title: Demo
paths:
  /users:
    get:
      summary: list
</file>
"#;
        let surface = introspect_repomix(Path::new("repomix-output.xml"), xml).unwrap();
        assert_eq!(surface.file_count("toml"), 1);
        assert_eq!(surface.file_count("yaml"), 1);
        assert_eq!(surface.file_count("sql"), 1);
        assert_eq!(surface.file_count("proto"), 1);
        assert_eq!(surface.file_count("graphql"), 1);
        assert_eq!(surface.file_count("openapi"), 1);

        assert!(surface
            .element_paths
            .iter()
            .any(|p| p == "toml.config.default.default_model"));
        assert!(surface
            .element_paths
            .iter()
            .any(|p| p.contains("sql.db.schema.table.users.email")));
        assert!(surface
            .element_paths
            .iter()
            .any(|p| p.contains("proto.api.v1.message.User.email")));
        assert!(surface
            .element_paths
            .iter()
            .any(|p| p.contains("graphql.schema.type.User.email")));
        assert!(surface
            .element_paths
            .iter()
            .any(|p| p.contains("openapi.") && p.contains("paths")));
    }
}
