//! Native operations on the bridge-owned cognitive Cozo database.
//!
//! Archives are versioned, tagged-value relation exports, not engine backups.
//! Cozo 0.7.6 does not expose default expressions or complete advanced-index
//! definitions; those and triggers/nonempty descriptions are rejected before
//! archive creation/restoration. Existing schemas must match an archive.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use op_cozo_store::{CozoGraphShuttle, DataValue, NamedRows, Num, ScriptMutability, Vector};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{
    BackupInput, DescribeRelationInput, ExportRelationsInput, ImportRelationsInput,
    ListRelationsInput, RestoreInput, RunQueryInput,
};

const FORMAT: &str = "opdbus-cozo-relations-v1";
const CODEC: &str = "opdbus-cozo-tagged-values-v1";
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ROWS: usize = 100_000;
const MAX_RELATIONS: usize = 256;

#[cfg(test)]
#[path = "cozo_methods_tests.rs"]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelationRows {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryOutput {
    pub results: Vec<RelationRows>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImportOutput {
    pub relations: Vec<String>,
    pub rows: u64,
    pub committed: bool,
    /// Imports use normal transactional puts, including their index/trigger semantics.
    pub triggers_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportOutput {
    /// Native Cozo JSON representation; backup uses a separate tagged codec
    /// to preserve Bytes/Uuid/vector values inside Any columns losslessly.
    pub encoding: String,
    pub data: BTreeMap<String, RelationRows>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RelationSummary {
    pub name: String,
    pub arity: u64,
    pub access_level: String,
    pub n_keys: u64,
    pub n_non_keys: u64,
    pub n_put_triggers: u64,
    pub n_rm_triggers: u64,
    pub n_replace_triggers: u64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ColumnDescription {
    pub column: String,
    pub is_key: bool,
    pub index: u64,
    #[serde(rename = "type")]
    pub type_name: String,
    pub has_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IndexDescription {
    pub name: String,
    #[serde(rename = "type")]
    pub index_type: String,
    pub relations: Vec<String>,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TriggerDescription {
    #[serde(rename = "type")]
    pub event: String,
    pub idx: u64,
    pub trigger: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelationDescription {
    pub relation: RelationSummary,
    pub columns: Vec<ColumnDescription>,
    pub indices: Vec<IndexDescription>,
    pub triggers: Vec<TriggerDescription>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListRelationsOutput {
    pub relations: Vec<RelationSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArchiveReceipt {
    pub archive: String,
    pub sha256: String,
    pub bytes: u64,
    pub relations: Vec<String>,
    pub rows: u64,
    pub format: String,
    pub scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RestoreReceipt {
    pub archive: String,
    pub sha256: String,
    pub relations: Vec<String>,
    pub rows: u64,
    pub committed: bool,
    pub atomic: bool,
    /// A native engine transaction commit; no claim of a SQLite engine image
    /// or power-loss durability beyond the configured Cozo storage engine.
    pub durability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelationArchive {
    format: String,
    value_codec: String,
    relations: Vec<ArchivedRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchivedRelation {
    metadata: RelationDescription,
    /// serde-tagged native DataValue cells, not display strings or JSON
    /// coercions. The versioned codec preserves Any-column value identities.
    rows: Vec<Vec<Value>>,
}

fn native_error(error: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("Cozo: {error}")
}

fn run(
    store: &CozoGraphShuttle,
    script: &str,
    params: BTreeMap<String, DataValue>,
    immutable: bool,
) -> Result<NamedRows> {
    store
        .db()
        .run_script(
            script,
            params,
            if immutable {
                ScriptMutability::Immutable
            } else {
                ScriptMutability::Mutable
            },
        )
        .map_err(native_error)
}

fn public_rows(rows: NamedRows) -> Result<RelationRows> {
    ensure!(
        rows.next.is_none(),
        "flatten native Cozo result chains before conversion"
    );
    Ok(RelationRows {
        headers: rows.headers,
        rows: rows
            .rows
            .into_iter()
            .map(|row| row.into_iter().map(Value::from).collect())
            .collect(),
    })
}

fn records<T: serde::de::DeserializeOwned>(rows: NamedRows) -> Result<Vec<T>> {
    let headers = rows.headers;
    rows.rows
        .into_iter()
        .map(|row| {
            ensure!(
                row.len() == headers.len(),
                "native Cozo row/header length mismatch"
            );
            let object: serde_json::Map<String, Value> = headers
                .iter()
                .cloned()
                .zip(row.into_iter().map(Value::from))
                .collect();
            Ok(serde_json::from_value(Value::Object(object))?)
        })
        .collect()
}

fn list_relations(store: &CozoGraphShuttle) -> Result<Vec<RelationSummary>> {
    records(run(store, "::relations", BTreeMap::new(), true)?)
}

fn simple_ident(name: &str) -> bool {
    static IDENTIFIER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^\p{XID_Start}[\p{XID_Continue}_]*$")
            .expect("Cozo identifier regex is valid")
    });
    IDENTIFIER.is_match(name)
}

fn relation_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= 256 && name.split('.').all(simple_ident),
        "relation name must contain Cozo identifier components"
    );
    Ok(())
}

fn checked_selection(names: &[String]) -> Result<()> {
    ensure!(
        !names.is_empty() && names.len() <= MAX_RELATIONS,
        "select 1..=256 relations"
    );
    let mut seen = BTreeSet::new();
    for name in names {
        relation_name(name)?;
        ensure!(seen.insert(name), "duplicate relation selection");
    }
    Ok(())
}

/// Schema and rows are captured by one immutable imperative transaction. A
/// preliminary list only supplies names/arity for safe query construction;
/// every actual descriptor used by the archive comes from this transaction.
fn snapshot(
    store: &CozoGraphShuttle,
    names: &[String],
    with_data: bool,
) -> Result<Vec<(RelationDescription, NamedRows)>> {
    checked_selection(names)?;
    let before = list_relations(store)?;
    let mut script = "{::relations} as _catalog\n".to_owned();
    let mut returns = vec!["_catalog".to_owned()];
    for (index, name) in names.iter().enumerate() {
        let info = before
            .iter()
            .find(|row| row.name == *name)
            .context("Cozo relation does not exist")?;
        ensure!(
            info.access_level != "index",
            "select the owning relation, not its internal index"
        );
        script.push_str(&format!("{{::columns {name}}} as _columns{index}\n{{::indices {name}}} as _indices{index}\n{{::show_triggers {name}}} as _triggers{index}\n"));
        returns.extend([
            format!("_columns{index}"),
            format!("_indices{index}"),
            format!("_triggers{index}"),
        ]);
        if with_data {
            ensure!(
                info.arity > 0 && info.arity <= 1024,
                "archive relation arity must be 1..=1024"
            );
            let vars = (0..info.arity)
                .map(|column| format!("v{column}"))
                .collect::<Vec<_>>()
                .join(",");
            script.push_str(&format!(
                "{{ ?[{vars}] := *{name}[{vars}] :limit {} }} as _data{index}\n",
                MAX_ROWS + 1
            ));
            returns.push(format!("_data{index}"));
        }
    }
    script.push_str(&format!("%return {}", returns.join(",")));
    let mut results = run(store, &script, BTreeMap::new(), true)?
        .flatten()
        .into_iter();
    let catalog: Vec<RelationSummary> =
        records(results.next().context("snapshot returned no catalog")?)?;
    let mut out = Vec::new();
    for name in names {
        let relation = catalog
            .iter()
            .find(|row| row.name == *name)
            .cloned()
            .context("relation changed before the snapshot")?;
        let mut columns: Vec<ColumnDescription> =
            records(results.next().context("snapshot omitted column metadata")?)?;
        columns.sort_by_key(|column| column.index);
        let indices = records(results.next().context("snapshot omitted index metadata")?)?;
        let triggers = records(
            results
                .next()
                .context("snapshot omitted trigger metadata")?,
        )?;
        let data = if with_data {
            results.next().context("snapshot omitted relation data")?
        } else {
            NamedRows::default()
        };
        out.push((
            RelationDescription {
                relation,
                columns,
                indices,
                triggers,
            },
            data,
        ));
    }
    ensure!(
        results.next().is_none(),
        "unexpected native snapshot result"
    );
    Ok(out)
}

fn supported_metadata(meta: &RelationDescription) -> Result<Vec<TypeSpec>> {
    let name = &meta.relation.name;
    relation_name(name)?;
    ensure!(
        matches!(
            meta.relation.access_level.as_str(),
            "normal" | "protected" | "read_only"
        ),
        "{name}: hidden/index relations cannot be portably archived"
    );
    ensure!(
        meta.relation.description.is_empty(),
        "{name}: Cozo 0.7.6 has no parsed description setter for faithful restore"
    );
    ensure!(
        meta.triggers.is_empty()
            && meta.relation.n_put_triggers == 0
            && meta.relation.n_rm_triggers == 0
            && meta.relation.n_replace_triggers == 0,
        "{name}: portable archive v1 does not restore executable triggers"
    );
    ensure!(
        !meta.columns.is_empty()
            && meta.columns.len() <= 1024
            && meta.columns.len() as u64 == meta.relation.arity,
        "{name}: invalid archive column count"
    );
    let mut names = BTreeSet::new();
    let mut types = Vec::new();
    for (index, column) in meta.columns.iter().enumerate() {
        ensure!(
            simple_ident(&column.column) && names.insert(&column.column),
            "{name}: invalid/duplicate column identifier"
        );
        ensure!(
            column.index == index as u64
                && column.is_key == (index < meta.relation.n_keys as usize),
            "{name}: column ordering/key metadata is inconsistent"
        );
        ensure!(
            !column.has_default,
            "{name}.{}: Cozo exposes has_default but not its expression; archive would lose schema behavior",
            column.column
        );
        types.push(TypeSpec::parse(&column.type_name)?);
    }
    ensure!(
        meta.relation.n_keys > 0
            && meta.relation.n_keys.checked_add(meta.relation.n_non_keys)
                == Some(meta.relation.arity),
        "{name}: invalid key/value column counts"
    );
    let mut index_names = BTreeSet::new();
    for index in &meta.indices {
        ensure!(
            index.index_type == "normal",
            "{name}: {} index {} has no complete portable reconstruction contract",
            index.index_type,
            index.name
        );
        ensure!(
            simple_ident(&index.name) && index_names.insert(&index.name),
            "{name}: invalid/duplicate index name"
        );
        let columns = index.config["indices"]
            .as_array()
            .context("normal index is missing column offsets")?;
        ensure!(
            !columns.is_empty()
                && columns.iter().all(|value| value
                    .as_u64()
                    .is_some_and(|offset| offset < meta.columns.len() as u64)),
            "{name}: invalid normal index column offsets"
        );
        ensure!(
            index
                .config
                .as_object()
                .is_some_and(|object| object.len() == 1),
            "{name}: unknown normal index metadata"
        );
        ensure!(
            index.relations == vec![format!("{name}:{}", index.name)],
            "{name}: unexpected backing relation for normal index"
        );
    }
    Ok(types)
}

fn archive_from_snapshot(
    snapshot: Vec<(RelationDescription, NamedRows)>,
) -> Result<RelationArchive> {
    let mut relations = Vec::new();
    let mut total = 0usize;
    for (metadata, data) in snapshot {
        let types = supported_metadata(&metadata)?;
        total = total
            .checked_add(data.rows.len())
            .context("archive row count overflow")?;
        ensure!(total <= MAX_ROWS, "portable archive exceeds 100000 rows");
        validate_rows(&metadata, &types, &data.rows)?;
        let rows = data
            .rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| {
                        let json = encode_archive_cell(&cell, 0)?;
                        let decoded = decode_archive_cell(&json, 0)?;
                        ensure!(
                            cell == decoded,
                            "Cozo value is not losslessly representable in the archive codec"
                        );
                        Ok(json)
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        relations.push(ArchivedRelation { metadata, rows });
    }
    Ok(RelationArchive {
        format: FORMAT.into(),
        value_codec: CODEC.into(),
        relations,
    })
}

fn decode_archive(
    archive: &RelationArchive,
) -> Result<Vec<(RelationDescription, Vec<Vec<DataValue>>)>> {
    ensure!(
        archive.format == FORMAT && archive.value_codec == CODEC,
        "unsupported Cozo relation archive format/codec"
    );
    let names = archive
        .relations
        .iter()
        .map(|relation| relation.metadata.relation.name.clone())
        .collect::<Vec<_>>();
    checked_selection(&names)?;
    let mut total = 0usize;
    archive
        .relations
        .iter()
        .map(|relation| {
            let types = supported_metadata(&relation.metadata)?;
            total = total
                .checked_add(relation.rows.len())
                .context("archive row count overflow")?;
            ensure!(total <= MAX_ROWS, "portable archive exceeds 100000 rows");
            let rows = relation
                .rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| decode_archive_cell(cell, 0))
                        .collect::<Result<Vec<_>>>()
                })
                .collect::<Result<Vec<_>>>()?;
            validate_rows(&relation.metadata, &types, &rows)?;
            Ok((relation.metadata.clone(), rows))
        })
        .collect()
}

// Cozo's Vector serde decoder requires borrowed native-endian bytes; it is
// not JSON-compatible. Store explicit IEEE bit words instead, including for
// vectors nested inside Any/List. Never invoke its unsafe byte decoder here.
fn encode_archive_cell(cell: &DataValue, depth: usize) -> Result<Value> {
    ensure!(depth <= 64, "archive value nesting exceeds 64 levels");
    match cell {
        DataValue::Vec(Vector::F32(values)) => {
            Ok(json!({"VecF32Bits":values.iter().map(|value| value.to_bits()).collect::<Vec<_>>()}))
        }
        DataValue::Vec(Vector::F64(values)) => {
            Ok(json!({"VecF64Bits":values.iter().map(|value| value.to_bits()).collect::<Vec<_>>()}))
        }
        DataValue::List(values) => Ok(
            json!({"List":values.iter().map(|value| encode_archive_cell(value, depth + 1)).collect::<Result<Vec<_>>>()?}),
        ),
        DataValue::Regex(_) | DataValue::Set(_) | DataValue::Bot => {
            bail!("internal Cozo values are not portable archive cells")
        }
        _ => Ok(serde_json::to_value(cell)?),
    }
}

fn decode_archive_cell(cell: &Value, depth: usize) -> Result<DataValue> {
    ensure!(depth <= 64, "archive value nesting exceeds 64 levels");
    if cell == "Null" {
        return Ok(DataValue::Null);
    }
    let object = cell
        .as_object()
        .context("invalid tagged Cozo archive cell")?;
    ensure!(
        object.len() == 1,
        "Cozo archive cell must have exactly one type tag"
    );
    let (tag, value) = object.iter().next().unwrap();
    match tag.as_str() {
        "List" => Ok(DataValue::List(
            value
                .as_array()
                .context("invalid Cozo list cell")?
                .iter()
                .map(|value| decode_archive_cell(value, depth + 1))
                .collect::<Result<Vec<_>>>()?,
        )),
        "VecF32Bits" | "VecF64Bits" => {
            let values = value.as_array().context("invalid Cozo vector bits")?;
            ensure!(
                values.len() <= 1_000_000,
                "Cozo vector exceeds archive dimension limit"
            );
            if tag == "VecF32Bits" {
                let values = values
                    .iter()
                    .map(|value| {
                        let bits = value
                            .as_u64()
                            .and_then(|bits| u32::try_from(bits).ok())
                            .context("invalid F32 bit word")?;
                        Ok(f32::from_bits(bits))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(DataValue::Vec(Vector::F32(values.into())))
            } else {
                let values = values
                    .iter()
                    .map(|value| {
                        Ok(f64::from_bits(
                            value.as_u64().context("invalid F64 bit word")?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(DataValue::Vec(Vector::F64(values.into())))
            }
        }
        // Regex's upstream deserializer panics; native Vec expects borrowed
        // bytes. Reject both before calling any upstream serde implementation.
        "Bool" | "Num" | "Str" | "Bytes" | "Uuid" | "Json" | "Validity" => {
            serde_json::from_value(cell.clone()).context("invalid tagged Cozo value")
        }
        _ => bail!("unsupported/internal Cozo archive cell tag: {tag}"),
    }
}

fn validate_rows(
    meta: &RelationDescription,
    types: &[TypeSpec],
    rows: &[Vec<DataValue>],
) -> Result<()> {
    // Preserve native key equality/order (including numeric semantics).
    // TypeSpec::accepts rejects Regex and other internal values recursively
    // before insertion, so no key here contains the regex cache Clippy flags.
    #[allow(clippy::mutable_key_type)]
    let mut keys = BTreeSet::new();
    for row in rows {
        ensure!(
            row.len() == types.len(),
            "archive row width does not match schema"
        );
        for (value, typ) in row.iter().zip(types) {
            ensure!(
                typ.accepts(value),
                "archive value does not match its declared Cozo type"
            );
        }
        ensure!(
            keys.insert(row[..meta.relation.n_keys as usize].to_vec()),
            "archive contains duplicate relation keys"
        );
    }
    Ok(())
}

/// Parse only Cozo's canonical metadata type grammar, not executable default
/// expressions. This makes every generated :create fragment non-injectable
/// and lets the entire archive be type-checked before the first write.
enum TypeKind {
    Scalar(String),
    List(Box<TypeSpec>, Option<usize>),
    Tuple(Vec<TypeSpec>),
    Vector { f32: bool, length: usize },
}

struct TypeSpec {
    kind: TypeKind,
    nullable: bool,
}

impl TypeSpec {
    fn parse(source: &str) -> Result<Self> {
        ensure!(
            !source.is_empty() && source.len() <= 1024,
            "invalid Cozo archive type length"
        );
        let mut position = 0;
        let parsed = Self::parse_inner(source.as_bytes(), &mut position, 0)?;
        ensure!(
            position == source.len(),
            "unsupported or malformed Cozo archive type"
        );
        Ok(parsed)
    }

    fn parse_inner(bytes: &[u8], position: &mut usize, depth: usize) -> Result<Self> {
        ensure!(depth <= 32, "Cozo archive type nesting is too deep");
        let kind = match bytes.get(*position).copied() {
            Some(b'[') => {
                *position += 1;
                let inner = Self::parse_inner(bytes, position, depth + 1)?;
                let length = if bytes.get(*position) == Some(&b';') {
                    *position += 1;
                    Some(type_length(bytes, position)?)
                } else {
                    None
                };
                ensure!(
                    bytes.get(*position) == Some(&b']'),
                    "invalid Cozo list type"
                );
                *position += 1;
                TypeKind::List(Box::new(inner), length)
            }
            Some(b'(') => {
                *position += 1;
                let mut elements = Vec::new();
                while bytes.get(*position) != Some(&b')') {
                    elements.push(Self::parse_inner(bytes, position, depth + 1)?);
                    if bytes.get(*position) != Some(&b',') {
                        break;
                    }
                    *position += 1;
                }
                ensure!(
                    bytes.get(*position) == Some(&b')'),
                    "invalid Cozo tuple type"
                );
                *position += 1;
                TypeKind::Tuple(elements)
            }
            Some(b'<') => {
                *position += 1;
                let tail = &bytes[*position..];
                let f32 = if tail.starts_with(b"F32;") {
                    true
                } else if tail.starts_with(b"F64;") {
                    false
                } else {
                    bail!("unsupported Cozo vector type");
                };
                *position += 4;
                let length = type_length(bytes, position)?;
                ensure!(
                    length > 0 && bytes.get(*position) == Some(&b'>'),
                    "invalid Cozo vector dimension"
                );
                *position += 1;
                TypeKind::Vector { f32, length }
            }
            _ => {
                let start = *position;
                while bytes.get(*position).is_some_and(u8::is_ascii_alphabetic) {
                    *position += 1;
                }
                let scalar = std::str::from_utf8(&bytes[start..*position])?;
                ensure!(
                    matches!(
                        scalar,
                        "Any"
                            | "Bool"
                            | "Int"
                            | "Float"
                            | "String"
                            | "Bytes"
                            | "Uuid"
                            | "Json"
                            | "Validity"
                    ),
                    "unsupported Cozo archive type {scalar}"
                );
                TypeKind::Scalar(scalar.to_owned())
            }
        };
        let nullable = bytes.get(*position) == Some(&b'?');
        if nullable {
            *position += 1;
        }
        Ok(Self { kind, nullable })
    }

    fn accepts(&self, value: &DataValue) -> bool {
        if matches!(value, DataValue::Null) {
            return self.nullable;
        }
        if !portable_value(value) {
            return false;
        }
        match (&self.kind, value) {
            (TypeKind::Scalar(name), _) if name == "Any" => true,
            (TypeKind::Scalar(name), DataValue::Bool(_)) => name == "Bool",
            (TypeKind::Scalar(name), DataValue::Num(Num::Int(_))) => name == "Int",
            (TypeKind::Scalar(name), DataValue::Num(Num::Float(_))) => name == "Float",
            (TypeKind::Scalar(name), DataValue::Str(_)) => name == "String",
            (TypeKind::Scalar(name), DataValue::Bytes(_)) => name == "Bytes",
            (TypeKind::Scalar(name), DataValue::Uuid(_)) => name == "Uuid",
            (TypeKind::Scalar(name), DataValue::Json(_)) => name == "Json",
            (TypeKind::Scalar(name), DataValue::Validity(_)) => name == "Validity",
            (TypeKind::List(inner, length), DataValue::List(values)) => {
                length.is_none_or(|length| length == values.len())
                    && values.iter().all(|value| inner.accepts(value))
            }
            (TypeKind::Tuple(types), DataValue::List(values)) => {
                types.len() == values.len()
                    && types
                        .iter()
                        .zip(values)
                        .all(|(typ, value)| typ.accepts(value))
            }
            (TypeKind::Vector { f32: true, length }, DataValue::Vec(Vector::F32(values))) => {
                *length == values.len()
            }
            (TypeKind::Vector { f32: false, length }, DataValue::Vec(Vector::F64(values))) => {
                *length == values.len()
            }
            _ => false,
        }
    }
}

fn type_length(bytes: &[u8], position: &mut usize) -> Result<usize> {
    let start = *position;
    while bytes.get(*position).is_some_and(u8::is_ascii_digit) {
        *position += 1;
    }
    let length = std::str::from_utf8(&bytes[start..*position])?
        .parse::<usize>()
        .context("invalid Cozo type dimension")?;
    ensure!(
        length <= 1_000_000,
        "Cozo type dimension exceeds archive limits"
    );
    Ok(length)
}

fn portable_value(value: &DataValue) -> bool {
    match value {
        DataValue::Num(Num::Float(number)) => number.is_finite(),
        DataValue::List(values) => values.iter().all(portable_value),
        DataValue::Vec(Vector::F32(values)) => values.iter().all(|number| number.is_finite()),
        DataValue::Vec(Vector::F64(values)) => values.iter().all(|number| number.is_finite()),
        DataValue::Regex(_) | DataValue::Set(_) | DataValue::Bot => false,
        _ => true,
    }
}

fn restore_archive(store: &CozoGraphShuttle, archive: &RelationArchive) -> Result<()> {
    // Decode/validate every schema, type, row and key before issuing any
    // write. No temporary database or alternate identity/project store opens.
    let decoded = decode_archive(archive)?;
    let existing = list_relations(store)?;
    let existing_names = decoded
        .iter()
        .map(|(meta, _)| meta.relation.name.clone())
        .filter(|name| existing.iter().any(|relation| relation.name == *name))
        .collect::<Vec<_>>();
    let existing_metadata = if existing_names.is_empty() {
        vec![]
    } else {
        snapshot(store, &existing_names, false)?
    };
    for (meta, _) in &decoded {
        if let Some((current, _)) = existing_metadata
            .iter()
            .find(|(current, _)| current.relation.name == meta.relation.name)
        {
            supported_metadata(current)?;
            ensure!(
                current == meta,
                "existing relation {} has different metadata; restore requires a matching schema or a missing relation",
                meta.relation.name
            );
        }
    }
    let mut script = String::new();
    let mut checks = "{::relations} as _restore_catalog\n".to_owned();
    let mut params = BTreeMap::new();
    for (index, (meta, rows)) in decoded.iter().enumerate() {
        let name = &meta.relation.name;
        let fields = meta
            .columns
            .iter()
            .map(|column| column.column.clone())
            .collect::<Vec<_>>();
        let all = fields.join(",");
        let keys = fields[..meta.relation.n_keys as usize].join(",");
        let values = fields[meta.relation.n_keys as usize..].join(",");
        let bindings = if values.is_empty() {
            keys.clone()
        } else {
            format!("{keys} => {values}")
        };
        if existing_names.contains(name) {
            append_metadata_checks(&mut checks, &mut params, index, meta)?;
            // No relation/index removal or :replace: Cozo's persisted-range
            // cleanup precedes commit, so those would weaken rollback safety.
            if meta.relation.access_level != "normal" {
                script.push_str(&format!("{{::access_level normal {name}}}\n"));
            }
            script.push_str(&format!(
                "{{ ?[{keys}] := *{name}{{{keys}}} :rm {name} {{{keys}}} }}\n"
            ));
            script.push_str(&format!(
                "{{ ?[{all}] <- $rows{index} :put {name} {{{bindings}}} }}\n"
            ));
        } else {
            let key_types = meta.columns[..meta.relation.n_keys as usize]
                .iter()
                .map(|column| format!("{}: {}", column.column, column.type_name))
                .collect::<Vec<_>>()
                .join(",");
            let value_types = meta.columns[meta.relation.n_keys as usize..]
                .iter()
                .map(|column| format!("{}: {}", column.column, column.type_name))
                .collect::<Vec<_>>()
                .join(",");
            let schema = if value_types.is_empty() {
                key_types
            } else {
                format!("{key_types} => {value_types}")
            };
            script.push_str(&format!(
                "{{ ?[{all}] <- $rows{index} :create {name} {{{schema}}} }}\n"
            ));
            for ordinary in &meta.indices {
                let columns = ordinary.config["indices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| fields[value.as_u64().unwrap() as usize].clone())
                    .collect::<Vec<_>>()
                    .join(",");
                script.push_str(&format!(
                    "{{::index create {name}:{} {{{columns}}}}}\n",
                    ordinary.name
                ));
            }
        }
        if meta.relation.access_level != "normal" {
            script.push_str(&format!(
                "{{::access_level {} {name}}}\n",
                meta.relation.access_level
            ));
        }
        params.insert(
            format!("rows{index}"),
            DataValue::List(rows.iter().cloned().map(DataValue::List).collect()),
        );
    }
    // The native imperative runner parses the whole script first, executes
    // its clauses in one engine transaction, and propagates commit failures.
    run(store, &format!("{checks}{script}"), params, false)?;
    Ok(())
}

fn append_metadata_checks(
    script: &mut String,
    params: &mut BTreeMap<String, DataValue>,
    index: usize,
    meta: &RelationDescription,
) -> Result<()> {
    let name = &meta.relation.name;
    params.insert(format!("relation{index}"), DataValue::from(name.as_str()));
    script.push_str(&format!("{{ ?[n,a,l,k,v,p,r,x,d] := *_restore_catalog[n,a,l,k,v,p,r,x,d], n = $relation{index} }} as _meta{index}\n"));
    script.push_str(&format!("{{::columns {name}}} as _check_columns{index}\n{{::indices {name}}} as _check_indices{index}\n{{::show_triggers {name}}} as _check_triggers{index}\n"));
    let relation = &meta.relation;
    let metadata = vec![vec![
        json!(relation.name),
        json!(relation.arity),
        json!(relation.access_level),
        json!(relation.n_keys),
        json!(relation.n_non_keys),
        json!(relation.n_put_triggers),
        json!(relation.n_rm_triggers),
        json!(relation.n_replace_triggers),
        json!(relation.description),
    ]];
    let columns = meta
        .columns
        .iter()
        .map(|column| {
            vec![
                json!(column.column),
                json!(column.is_key),
                json!(column.index),
                json!(column.type_name),
                json!(column.has_default),
            ]
        })
        .collect();
    let indices = meta
        .indices
        .iter()
        .map(|index| {
            vec![
                json!(index.name),
                json!(index.index_type),
                json!(index.relations),
                index.config.clone(),
            ]
        })
        .collect();
    append_row_equality(script, params, &format!("_meta{index}"), 9, metadata);
    append_row_equality(
        script,
        params,
        &format!("_check_columns{index}"),
        5,
        columns,
    );
    append_row_equality(
        script,
        params,
        &format!("_check_indices{index}"),
        4,
        indices,
    );
    append_row_equality(
        script,
        params,
        &format!("_check_triggers{index}"),
        3,
        meta.triggers
            .iter()
            .map(|trigger| {
                vec![
                    json!(trigger.event),
                    json!(trigger.idx),
                    json!(trigger.trigger),
                ]
            })
            .collect(),
    );
    Ok(())
}

fn append_row_equality(
    script: &mut String,
    params: &mut BTreeMap<String, DataValue>,
    table: &str,
    arity: usize,
    expected: Vec<Vec<Value>>,
) {
    let parameter = format!("expected{table}");
    let variables = (0..arity)
        .map(|index| format!("c{index}"))
        .collect::<Vec<_>>()
        .join(",");
    params.insert(
        parameter.clone(),
        DataValue::List(
            expected
                .into_iter()
                .map(|row| DataValue::List(row.into_iter().map(DataValue::from).collect()))
                .collect(),
        ),
    );
    // Symmetric difference must be empty. These assertions execute inside the
    // same transaction, before any row writes, closing the preflight/DDL race.
    script.push_str(&format!("{{ expected[{variables}] <- ${parameter}\n ?[{variables}] := *{table}[{variables}], not expected[{variables}]\n ?[{variables}] := expected[{variables}], not *{table}[{variables}]\n :assert none }}\n"));
}

fn archive_dir(root: &Path) -> Result<PathBuf> {
    if !root.exists() {
        fs::create_dir_all(root)?;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    let metadata = fs::symlink_metadata(root)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Cozo archive root must be a real directory"
    );
    ensure!(
        metadata.uid() == nix::unistd::geteuid().as_raw() && metadata.mode() & 0o022 == 0,
        "Cozo archive root must be owned by this service and not group/world writable"
    );
    fs::canonicalize(root).context("cannot resolve Cozo archive directory")
}

fn write_archive(root: &Path, label: &str, archive: &RelationArchive) -> Result<ArchiveReceipt> {
    ensure!(
        !label.is_empty()
            && label.len() <= 64
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')),
        "backup path is a short archive label, not a filesystem path"
    );
    let bytes = serde_json::to_vec(archive)?;
    ensure!(
        bytes.len() as u64 <= MAX_ARCHIVE_BYTES,
        "Cozo archive exceeds 64 MiB"
    );
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let root = archive_dir(root)?;
    let id = uuid::Uuid::new_v4();
    let filename = format!("{label}-{id}.{hash}.cozo.json");
    let temporary = root.join(format!(".tmp-{id}"));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o400)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        // hard_link is an atomic, no-overwrite publication on this filesystem.
        fs::hard_link(&temporary, root.join(&filename))?;
        fs::remove_file(&temporary)?;
        File::open(&root)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(ArchiveReceipt {
        archive: filename,
        sha256: hash,
        bytes: bytes.len() as u64,
        relations: archive
            .relations
            .iter()
            .map(|relation| relation.metadata.relation.name.clone())
            .collect(),
        rows: archive
            .relations
            .iter()
            .map(|relation| relation.rows.len() as u64)
            .sum(),
        format: FORMAT.into(),
        scope: "selected relations; not an engine or whole-host backup".into(),
    })
}

fn read_archive(root: &Path, filename: &str) -> Result<(RelationArchive, String)> {
    ensure!(
        filename.len() <= 256
            && filename
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "restore requires an archive receipt basename"
    );
    let prefix = filename
        .strip_suffix(".cozo.json")
        .context("invalid Cozo archive suffix")?;
    let (_, hash) = prefix
        .rsplit_once('.')
        .context("archive name has no content digest")?;
    ensure!(
        hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid archive content digest"
    );
    let root = archive_dir(root)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(root.join(filename))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == nix::unistd::geteuid().as_raw()
            && metadata.mode() & 0o222 == 0
            && metadata.len() <= MAX_ARCHIVE_BYTES,
        "restore requires a bounded immutable archive owned by this service"
    );
    let mut bytes = Vec::new();
    file.take(MAX_ARCHIVE_BYTES + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_ARCHIVE_BYTES,
        "Cozo archive exceeds 64 MiB"
    );
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == hash,
        "Cozo archive content digest mismatch"
    );
    Ok((
        serde_json::from_slice(&bytes).context("invalid Cozo relation archive")?,
        hash.to_owned(),
    ))
}

pub async fn dispatch_cozo_method(
    store: Arc<CozoGraphShuttle>,
    method: &str,
    args: &Value,
) -> Result<Value> {
    let root = std::env::var_os("OP_COZO_ARCHIVE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/var/lib/opdbus/cozo-archives".into());
    dispatch_cozo_with_archive_dir(store, root, method, args).await
}

pub async fn dispatch_cozo_with_archive_dir(
    store: Arc<CozoGraphShuttle>,
    archive_root: PathBuf,
    method: &str,
    args: &Value,
) -> Result<Value> {
    let method = method.to_owned();
    let args = args.clone();
    tokio::task::spawn_blocking(move || dispatch_sync(&store, &archive_root, &method, &args))
        .await?
}

fn dispatch_sync(
    store: &CozoGraphShuttle,
    root: &Path,
    method: &str,
    args: &Value,
) -> Result<Value> {
    match method {
        "list_system_graphs" => {
            let input = serde_json::from_value(args.clone())?;
            Ok(serde_json::to_value(store.list_system_graphs(&input)?)?)
        }
        "replace_system_graph" => {
            let input: super::ReplaceSystemGraphInput = serde_json::from_value(args.clone())?;
            ensure!(
                !input.snapshot.namespace.starts_with("runtime:")
                    && input.snapshot.provenance.kind
                        != op_cozo_store::GraphProvenanceKind::ObservedRuntime,
                "observed runtime snapshots are owned by refresh_system_graph"
            );
            ensure!(
                input
                    .snapshot
                    .edges
                    .iter()
                    .all(|edge| edge.evidence.iter().all(|evidence| {
                        evidence.assertion != op_cozo_store::GraphAssertion::Observed
                    })),
                "imported inventories may declare or infer relationships; observed evidence requires refresh_system_graph"
            );
            Ok(serde_json::to_value(store.replace_system_graph(
                &input.snapshot,
                input.expected_revision,
            )?)?)
        }
        "system_graph_status" => {
            let input: super::SystemGraphStatusInput = serde_json::from_value(args.clone())?;
            Ok(serde_json::to_value(super::SystemGraphStatusOutput {
                snapshot: store.system_graph_status(&input.namespace)?,
            })?)
        }
        "query_system_graph_nodes" => {
            let input = serde_json::from_value(args.clone())?;
            Ok(serde_json::to_value(
                store.query_system_graph_nodes(&input)?,
            )?)
        }
        "query_system_graph_edges" => {
            let input = serde_json::from_value(args.clone())?;
            Ok(serde_json::to_value(
                store.query_system_graph_edges(&input)?,
            )?)
        }
        "traverse_system_graph" => {
            let input = serde_json::from_value(args.clone())?;
            Ok(serde_json::to_value(store.traverse_system_graph(&input)?)?)
        }
        "run_query" => {
            let input: RunQueryInput = serde_json::from_value(args.clone())?;
            ensure!(
                !input.query.trim().is_empty() && input.query.len() <= 1024 * 1024,
                "Cozo query must contain 1..=1048576 bytes"
            );
            let params = input
                .params
                .into_iter()
                .map(|(name, value)| (name, DataValue::from(value)))
                .collect();
            let results = run(store, &input.query, params, false)?
                .flatten()
                .into_iter()
                .map(public_rows)
                .collect::<Result<_>>()?;
            Ok(serde_json::to_value(QueryOutput { results })?)
        }
        "list_relations" => {
            serde_json::from_value::<ListRelationsInput>(args.clone())?;
            Ok(serde_json::to_value(ListRelationsOutput {
                relations: list_relations(store)?,
            })?)
        }
        "describe_relation" => {
            let input: DescribeRelationInput = serde_json::from_value(args.clone())?;
            let (metadata, _) = snapshot(store, &[input.name], false)?
                .into_iter()
                .next()
                .unwrap();
            Ok(serde_json::to_value(metadata)?)
        }
        "export_relations" => {
            let input: ExportRelationsInput = serde_json::from_value(args.clone())?;
            checked_selection(&input.relations)?;
            let rows = store
                .db()
                .export_relations(input.relations.iter())
                .map_err(native_error)?;
            let data = rows
                .into_iter()
                .map(|(name, rows)| Ok((name, public_rows(rows)?)))
                .collect::<Result<_>>()?;
            Ok(serde_json::to_value(ExportOutput {
                encoding: "cozo-json".into(),
                data,
            })?)
        }
        "import_relations" => {
            let input: ImportRelationsInput = serde_json::from_value(args.clone())?;
            let names = input.data.keys().cloned().collect::<Vec<_>>();
            checked_selection(&names)?;
            let metadata = snapshot(store, &names, false)?;
            let mut checks = "{::relations} as _restore_catalog\n".to_owned();
            let mut writes = String::new();
            let mut count = 0u64;
            let mut params = BTreeMap::new();
            for (index, (name, rows)) in input.data.into_iter().enumerate() {
                let meta = &metadata
                    .iter()
                    .find(|(meta, _)| meta.relation.name == name)
                    .context("import target metadata is missing")?
                    .0;
                ensure!(
                    !rows.headers.is_empty()
                        && rows.headers.iter().collect::<BTreeSet<_>>().len() == rows.headers.len(),
                    "import headers must be nonempty and unique"
                );
                let columns = meta
                    .columns
                    .iter()
                    .map(|column| column.column.as_str())
                    .collect::<BTreeSet<_>>();
                ensure!(
                    rows.headers.iter().all(|header| simple_ident(header))
                        && rows
                            .headers
                            .iter()
                            .map(String::as_str)
                            .collect::<BTreeSet<_>>()
                            == columns,
                    "import headers must exactly name the target relation columns"
                );
                ensure!(
                    rows.rows.iter().all(|row| row.len() == rows.headers.len()),
                    "import row/header length mismatch"
                );
                count += rows.rows.len() as u64;
                ensure!(
                    count <= MAX_ROWS as u64,
                    "native import exceeds 100000 rows"
                );
                append_metadata_checks(&mut checks, &mut params, index, meta)?;
                let keys = meta
                    .columns
                    .iter()
                    .filter(|column| column.is_key)
                    .map(|column| column.column.clone())
                    .collect::<Vec<_>>()
                    .join(",");
                let values = meta
                    .columns
                    .iter()
                    .filter(|column| !column.is_key)
                    .map(|column| column.column.clone())
                    .collect::<Vec<_>>()
                    .join(",");
                let bindings = if values.is_empty() {
                    keys
                } else {
                    format!("{keys} => {values}")
                };
                writes.push_str(&format!(
                    "{{ ?[{}] <- $import{index} :put {name} {{{bindings}}} }}\n",
                    rows.headers.join(",")
                ));
                params.insert(
                    format!("import{index}"),
                    DataValue::List(
                        rows.rows
                            .into_iter()
                            .map(|row| {
                                DataValue::List(row.into_iter().map(DataValue::from).collect())
                            })
                            .collect(),
                    ),
                );
            }
            // The bulk import helper skips advanced indexes and callbacks in
            // Cozo 0.7.6. Native :put programs preserve normal write semantics.
            run(store, &format!("{checks}{writes}"), params, false)?;
            Ok(serde_json::to_value(ImportOutput {
                relations: names,
                rows: count,
                committed: true,
                triggers_enabled: true,
            })?)
        }
        "backup" => {
            let input: BackupInput = serde_json::from_value(args.clone())?;
            let names = if input.relations.is_empty() {
                list_relations(store)?
                    .into_iter()
                    .filter(|relation| relation.access_level != "index")
                    .map(|relation| relation.name)
                    .collect()
            } else {
                input.relations
            };
            let archive = archive_from_snapshot(snapshot(store, &names, true)?)?;
            Ok(serde_json::to_value(write_archive(
                root,
                &input.path,
                &archive,
            )?)?)
        }
        "restore" => {
            let input: RestoreInput = serde_json::from_value(args.clone())?;
            let (archive, hash) = read_archive(root, &input.path)?;
            restore_archive(store, &archive)?;
            Ok(serde_json::to_value(RestoreReceipt {
                archive: input.path,
                sha256: hash,
                relations: archive
                    .relations
                    .iter()
                    .map(|relation| relation.metadata.relation.name.clone())
                    .collect(),
                rows: archive
                    .relations
                    .iter()
                    .map(|relation| relation.rows.len() as u64)
                    .sum(),
                committed: true,
                atomic: true,
                durability: "native_engine_transaction_commit".into(),
            })?)
        }
        _ => bail!("unknown cozo method: {method}"),
    }
}
