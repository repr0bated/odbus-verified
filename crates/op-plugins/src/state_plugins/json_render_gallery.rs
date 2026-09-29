//! The UI gallery and the active chat Canvas catalog, owned by json_render.
//!
//! A *candidate* is a canvas a human kept from a chat reply (normally UI
//! render mode). Candidates are private to the principal who saved them and
//! unbounded: the store keeps one row each and reads them a page at a time.
//!
//! The *catalog* is system-wide: a promoted candidate becomes an entry every
//! human can read, and the normal chat modes compose from it. Promotion
//! copies the validated canvas; the candidate keeps a link to its entry.
//! Retiring an entry removes it from the active catalog without deleting its
//! record. Promotion adds compositions of existing Canvas components; a new
//! component type still needs UI code and a profile import.
//!
//! This module is the contract: typed inputs and outputs, their checks and
//! the method declarations. MutationEngine owns identity, grants, the
//! durable store and canvas validation against the sealed profile.
use anyhow::{ensure, Result};
use op_state_store::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

pub const GALLERY_CAPABILITY: &str = "json_render.gallery";
pub const PROMOTE_CAPABILITY: &str = "json_render.promote";
pub const MAX_TITLE_BYTES: usize = 200;
pub const MAX_DESCRIPTION_BYTES: usize = 2000;
pub const MAX_PROMPT_BYTES: usize = 8 * 1024;
pub const DEFAULT_PAGE: u32 = 50;
pub const MAX_PAGE: u32 = 100;
/// Chat modes a candidate can come from (the UI's ChatMode).
pub const MODES: [&str; 6] = [
    "personal",
    "project",
    "control_plane",
    "accountability",
    "notebook_lm",
    "ui_render",
];

pub const METHODS: [&str; 8] = [
    "save_candidate",
    "list_candidates",
    "get_candidate",
    "delete_candidate",
    "promote_candidate",
    "list_catalog",
    "get_catalog_entry",
    "retire_catalog_entry",
];

pub fn is_gallery_method(method: &str) -> bool {
    METHODS.contains(&method)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveCandidateInput {
    /// A chat Canvas: exactly `{root, elements}`.
    pub spec: Value,
    pub title: String,
    /// The chat mode the canvas came from.
    pub mode: String,
    /// The request that produced it, if the UI keeps it.
    #[serde(default)]
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateRefInput {
    pub candidate_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageInput {
    /// Return items older than this ID (the previous page's `next_before`).
    #[serde(default)]
    pub before: Option<String>,
    /// Page size; 0 means the default.
    #[serde(default)]
    pub limit: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogPageInput {
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub limit: u32,
    /// Include retired entries.
    #[serde(default)]
    pub include_retired: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PromoteInput {
    pub candidate_id: String,
    /// Catalog name, unique among active entries.
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntryRefInput {
    pub entry_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetireInput {
    pub entry_id: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub candidate_id: String,
    pub principal_id: String,
    pub title: String,
    pub mode: String,
    #[serde(default)]
    pub prompt: Option<String>,
    pub spec: Value,
    /// sha256 of the canonical canvas.
    pub spec_hash: String,
    /// The chat Canvas profile export it was validated against.
    pub profile_sha256: String,
    pub created_at: String,
    /// The catalog entry this candidate became, once promoted.
    #[serde(default)]
    pub promoted_entry_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateSummary {
    pub candidate_id: String,
    pub title: String,
    pub mode: String,
    pub spec_hash: String,
    pub created_at: String,
    #[serde(default)]
    pub promoted_entry_id: Option<String>,
}

impl From<&Candidate> for CandidateSummary {
    fn from(candidate: &Candidate) -> Self {
        Self {
            candidate_id: candidate.candidate_id.clone(),
            title: candidate.title.clone(),
            mode: candidate.mode.clone(),
            spec_hash: candidate.spec_hash.clone(),
            created_at: candidate.created_at.clone(),
            promoted_entry_id: candidate.promoted_entry_id.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidateOutput {
    pub candidate: Candidate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidatePage {
    /// Newest first.
    pub candidates: Vec<CandidateSummary>,
    /// Pass as `before` for the next page; absent on the last page.
    #[serde(default)]
    pub next_before: Option<String>,
    /// All of the caller's candidates.
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeletedOutput {
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    pub entry_id: String,
    pub name: String,
    pub description: String,
    pub spec: Value,
    pub spec_hash: String,
    pub profile_sha256: String,
    pub from_candidate_id: String,
    pub promoted_by: String,
    pub promoted_at: String,
    /// `active` or `retired`.
    pub status: String,
    #[serde(default)]
    pub retired_at: Option<String>,
    #[serde(default)]
    pub retired_by: Option<String>,
    #[serde(default)]
    pub retire_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntrySummary {
    pub entry_id: String,
    pub name: String,
    pub description: String,
    pub spec_hash: String,
    pub promoted_at: String,
    pub status: String,
}

impl From<&CatalogEntry> for CatalogEntrySummary {
    fn from(entry: &CatalogEntry) -> Self {
        Self {
            entry_id: entry.entry_id.clone(),
            name: entry.name.clone(),
            description: entry.description.clone(),
            spec_hash: entry.spec_hash.clone(),
            promoted_at: entry.promoted_at.clone(),
            status: entry.status.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntryOutput {
    pub entry: CatalogEntry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogPage {
    /// Newest promotion first.
    pub entries: Vec<CatalogEntrySummary>,
    #[serde(default)]
    pub next_before: Option<String>,
    /// Active entries (all entries when retired ones are included).
    pub total: u64,
}

fn text(what: &str, value: &str, minimum: usize, maximum: usize, multiline: bool) -> Result<()> {
    ensure!(
        value.len() >= minimum && value.len() <= maximum && value.trim() == value,
        "{what} must be {minimum} to {maximum} bytes without surrounding whitespace"
    );
    ensure!(
        !value
            .chars()
            .any(|ch| ch.is_control() && !(multiline && ch == '\n')),
        "{what} contains control characters"
    );
    Ok(())
}

pub fn validate_uuid_v7(what: &str, value: &str) -> Result<()> {
    let parsed =
        uuid::Uuid::parse_str(value).map_err(|_| anyhow::anyhow!("{what} is not a UUID"))?;
    ensure!(
        parsed.get_version_num() == 7 && parsed.hyphenated().to_string() == value,
        "{what} must be a canonical UUIDv7"
    );
    Ok(())
}

pub fn validate_hash(what: &str, value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "{what} must be a sha256 hex digest"
    );
    Ok(())
}

fn validate_time(what: &str, value: &str) -> Result<()> {
    chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| anyhow::anyhow!("{what} must be an RFC 3339 time"))?;
    Ok(())
}

pub fn validate_title(title: &str) -> Result<()> {
    text("title", title, 1, MAX_TITLE_BYTES, false)
}

pub fn validate_name(name: &str) -> Result<()> {
    text("catalog name", name, 1, MAX_TITLE_BYTES, false)
}

pub fn validate_description(description: &str) -> Result<()> {
    text("description", description, 0, MAX_DESCRIPTION_BYTES, true)
}

pub fn validate_reason(reason: &str) -> Result<()> {
    text("retire reason", reason, 0, MAX_DESCRIPTION_BYTES, true)
}

pub fn validate_mode(mode: &str) -> Result<()> {
    ensure!(MODES.contains(&mode), "unknown chat mode {mode:?}");
    Ok(())
}

pub fn validate_prompt(prompt: Option<&str>) -> Result<()> {
    if let Some(prompt) = prompt {
        ensure!(
            !prompt.is_empty() && prompt.len() <= MAX_PROMPT_BYTES,
            "prompt must be 1 to {MAX_PROMPT_BYTES} bytes"
        );
        ensure!(
            !prompt
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t')),
            "prompt contains control characters"
        );
    }
    Ok(())
}

/// The effective page size of a request.
pub fn page_limit(limit: u32) -> Result<usize> {
    ensure!(limit <= MAX_PAGE, "page limit exceeds {MAX_PAGE}");
    Ok(if limit == 0 { DEFAULT_PAGE } else { limit } as usize)
}

/// Deterministic JSON bytes: object keys sorted at every level.
pub fn canonical_json(value: &Value) -> Vec<u8> {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(object) => {
                let mut keys: Vec<_> = object.keys().collect();
                keys.sort();
                Value::Object(
                    keys.into_iter()
                        .map(|key| (key.clone(), sorted(&object[key])))
                        .collect(),
                )
            }
            Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_vec(&sorted(value)).expect("JSON values serialize")
}

/// `sha256(canonical(spec))`.
pub fn spec_hash(spec: &Value) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(canonical_json(spec)))
}

/// Structural checks of a stored candidate; the canvas itself is checked
/// against the sealed profile by the caller.
pub fn validate_candidate(candidate: &Candidate) -> Result<()> {
    validate_uuid_v7("candidate_id", &candidate.candidate_id)?;
    ensure!(
        !candidate.principal_id.is_empty() && candidate.principal_id.len() <= 512,
        "invalid candidate principal"
    );
    validate_title(&candidate.title)?;
    validate_mode(&candidate.mode)?;
    validate_prompt(candidate.prompt.as_deref())?;
    validate_hash("spec_hash", &candidate.spec_hash)?;
    validate_hash("profile_sha256", &candidate.profile_sha256)?;
    ensure!(
        spec_hash(&candidate.spec) == candidate.spec_hash,
        "candidate spec differs from its spec_hash"
    );
    validate_time("created_at", &candidate.created_at)?;
    if let Some(entry) = &candidate.promoted_entry_id {
        validate_uuid_v7("promoted_entry_id", entry)?;
    }
    Ok(())
}

pub fn validate_entry(entry: &CatalogEntry) -> Result<()> {
    validate_uuid_v7("entry_id", &entry.entry_id)?;
    validate_uuid_v7("from_candidate_id", &entry.from_candidate_id)?;
    validate_name(&entry.name)?;
    validate_description(&entry.description)?;
    validate_hash("spec_hash", &entry.spec_hash)?;
    validate_hash("profile_sha256", &entry.profile_sha256)?;
    ensure!(
        spec_hash(&entry.spec) == entry.spec_hash,
        "catalog entry spec differs from its spec_hash"
    );
    ensure!(
        !entry.promoted_by.is_empty() && entry.promoted_by.len() <= 512,
        "invalid catalog promoter"
    );
    validate_time("promoted_at", &entry.promoted_at)?;
    match entry.status.as_str() {
        "active" => ensure!(
            entry.retired_at.is_none()
                && entry.retired_by.is_none()
                && entry.retire_reason.is_none(),
            "an active catalog entry has no retirement"
        ),
        "retired" => {
            validate_time(
                "retired_at",
                entry
                    .retired_at
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("a retired entry records when"))?,
            )?;
            ensure!(
                entry
                    .retired_by
                    .as_deref()
                    .is_some_and(|by| !by.is_empty() && by.len() <= 512),
                "a retired entry records who retired it"
            );
            validate_reason(entry.retire_reason.as_deref().unwrap_or_default())?;
        }
        _ => anyhow::bail!("catalog entry status must be active or retired"),
    }
    Ok(())
}

pub(crate) fn declare_methods(schema: &mut PluginSchema) {
    macro_rules! method {
        ($name:literal, $input:ty, $output:ty, $effect:ident, $idem:literal, $cap:expr, $subid:literal) => {
            schema.methods.insert(
                $name.into(),
                method_decl_from_schemars_with_output::<$input, $output>(
                    $name,
                    SideEffect::$effect,
                    $idem,
                    $cap,
                    $subid,
                ),
            );
        };
    }
    method!(
        "save_candidate",
        SaveCandidateInput,
        CandidateOutput,
        Mutation,
        false,
        GALLERY_CAPABILITY,
        "exp.software.plugin.json-render.gallery.save@v1"
    );
    method!(
        "list_candidates",
        PageInput,
        CandidatePage,
        Read,
        true,
        GALLERY_CAPABILITY,
        "obs.software.plugin.json-render.gallery.list@v1"
    );
    method!(
        "get_candidate",
        CandidateRefInput,
        CandidateOutput,
        Read,
        true,
        GALLERY_CAPABILITY,
        "obs.software.plugin.json-render.gallery.get@v1"
    );
    method!(
        "delete_candidate",
        CandidateRefInput,
        DeletedOutput,
        Mutation,
        true,
        GALLERY_CAPABILITY,
        "exp.software.plugin.json-render.gallery.delete@v1"
    );
    method!(
        "promote_candidate",
        PromoteInput,
        CatalogEntryOutput,
        Mutation,
        false,
        PROMOTE_CAPABILITY,
        "exp.software.plugin.json-render.catalog.promote@v1"
    );
    method!(
        "list_catalog",
        CatalogPageInput,
        CatalogPage,
        Read,
        true,
        GALLERY_CAPABILITY,
        "obs.software.plugin.json-render.catalog.list@v1"
    );
    method!(
        "get_catalog_entry",
        CatalogEntryRefInput,
        CatalogEntryOutput,
        Read,
        true,
        GALLERY_CAPABILITY,
        "obs.software.plugin.json-render.catalog.get@v1"
    );
    method!(
        "retire_catalog_entry",
        RetireInput,
        CatalogEntryOutput,
        Mutation,
        true,
        PROMOTE_CAPABILITY,
        "exp.software.plugin.json-render.catalog.retire@v1"
    );
    schema.capabilities.insert(
        GALLERY_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: GALLERY_CAPABILITY.to_string(),
            description: "Grants: save_candidate, list_candidates, get_candidate, delete_candidate (the caller's own candidates), list_catalog, get_catalog_entry.".to_string(),
        },
    );
    schema.capabilities.insert(
        PROMOTE_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: PROMOTE_CAPABILITY.to_string(),
            description: "Grants: promote_candidate (the caller's own candidates into the system catalog), retire_catalog_entry.".to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn candidate() -> Candidate {
        let spec = json!({"root": "a", "elements": {"a": {"type": "text", "props": {"text": "x", "className": null}}}});
        Candidate {
            candidate_id: uuid::Uuid::now_v7().to_string(),
            principal_id: "did:op:human:one".into(),
            title: "Proposal".into(),
            mode: "ui_render".into(),
            prompt: Some("draw a status page".into()),
            spec_hash: spec_hash(&spec),
            spec,
            profile_sha256: "a".repeat(64),
            created_at: chrono::Utc::now().to_rfc3339(),
            promoted_entry_id: None,
        }
    }

    #[test]
    fn spec_hash_ignores_key_order() {
        let one = json!({"root": "a", "elements": {"a": {"type": "text", "props": {"text": "x", "className": null}}}});
        let two = json!({"elements": {"a": {"props": {"className": null, "text": "x"}, "type": "text"}}, "root": "a"});
        assert_eq!(spec_hash(&one), spec_hash(&two));
    }

    #[test]
    fn candidate_checks() {
        validate_candidate(&candidate()).unwrap();
        let mut tampered = candidate();
        tampered.spec["root"] = json!("b");
        assert!(validate_candidate(&tampered).is_err());
        let mut mode = candidate();
        mode.mode = "admin".into();
        assert!(validate_candidate(&mode).is_err());
        let mut title = candidate();
        title.title = " padded".into();
        assert!(validate_candidate(&title).is_err());
        let mut v4 = candidate();
        v4.candidate_id = uuid::Uuid::new_v4().to_string();
        assert!(validate_candidate(&v4).is_err());
    }

    #[test]
    fn entry_status_must_match_retirement() {
        let candidate = candidate();
        let mut entry = CatalogEntry {
            entry_id: uuid::Uuid::now_v7().to_string(),
            name: "Status page".into(),
            description: "Line one\nline two".into(),
            spec: candidate.spec.clone(),
            spec_hash: candidate.spec_hash.clone(),
            profile_sha256: candidate.profile_sha256.clone(),
            from_candidate_id: candidate.candidate_id.clone(),
            promoted_by: candidate.principal_id.clone(),
            promoted_at: chrono::Utc::now().to_rfc3339(),
            status: "active".into(),
            retired_at: None,
            retired_by: None,
            retire_reason: None,
        };
        validate_entry(&entry).unwrap();
        entry.status = "retired".into();
        assert!(validate_entry(&entry).is_err());
        entry.retired_at = Some(chrono::Utc::now().to_rfc3339());
        entry.retired_by = Some("did:op:human:one".into());
        validate_entry(&entry).unwrap();
        entry.status = "draft".into();
        assert!(validate_entry(&entry).is_err());
    }

    #[test]
    fn page_limits() {
        assert_eq!(page_limit(0).unwrap(), DEFAULT_PAGE as usize);
        assert_eq!(page_limit(MAX_PAGE).unwrap(), MAX_PAGE as usize);
        assert!(page_limit(MAX_PAGE + 1).is_err());
    }

    #[test]
    fn methods_are_declared_with_their_grants() {
        let schema = crate::state_plugins::json_render::json_render_schema();
        for method in METHODS {
            let declaration = schema.methods.get(method).unwrap();
            let expected = if matches!(method, "promote_candidate" | "retire_catalog_entry") {
                PROMOTE_CAPABILITY
            } else {
                GALLERY_CAPABILITY
            };
            assert_eq!(
                declaration.required_capability.as_deref(),
                Some(expected),
                "{method}"
            );
        }
        assert!(schema.capabilities.contains_key(GALLERY_CAPABILITY));
        assert!(schema.capabilities.contains_key(PROMOTE_CAPABILITY));
        assert!(schema.fields.contains_key("chat_canvas_profile"));
    }
}
