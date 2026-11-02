/// Closed grant groups. A principal belongs to zero or more of these and
/// inherits their capabilities. The names are not caller ids.
pub const GRANT_GROUPS: &[&str] = &[
    "$ROOT",
    "$ADMIN",
    "$SUPERADMIN",
    "$EXTERNAL-MCP",
    "$VENDOR",
    "$GUEST",
    "$AGENT-L1",
    "$AGENT-L2",
    "$AGENT-L3",
    "$AGENT-L4",
];

fn legacy_footprint_key(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn grant_document_rejected(keys: impl Iterator<Item = impl AsRef<str>>) -> bool {
    keys.into_iter().any(|key| {
        let key = key.as_ref();
        key == "*"
            || legacy_footprint_key(key)
            || (key.starts_with('$') && !GRANT_GROUPS.contains(&key))
    })
}

/// Validate a durable grant document.
///
/// Principal entries keep an exact id and a `capabilities` array. Optional
/// `groups` names must be members of [`GRANT_GROUPS`] and must exist as
/// entries in the same document. A `*` key, a legacy 64-hex footprint key, or
/// an unknown `$` name rejects the document.
pub fn validate_grant_document(bytes: &[u8]) -> Result<(), String> {
    let document: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("grant document is not JSON: {error}"))?;
    validated_grant_entries(&document).map(|_| ())
}

/// Share the complete document contract between publication and every reader.
/// Runtime expansion must not salvage authority from a document the materializer
/// would reject, including malformed entries for other principals or groups.
pub fn validated_grant_entries(
    document: &serde_json::Value,
) -> Result<&serde_json::Map<String, serde_json::Value>, String> {
    let entries = document.as_object().ok_or_else(|| {
        "grant document must be a JSON object keyed by principal_id or group".to_string()
    })?;
    if grant_document_rejected(entries.keys()) {
        return Err("prohibited wildcard, legacy footprint, or unknown group key".to_string());
    }
    for (key, entry) in entries {
        let entry = entry
            .as_object()
            .ok_or_else(|| format!("grant entry for {key} must be an object"))?;
        let capabilities = entry
            .get("capabilities")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| format!("grant entry for {key} must own a capabilities array"))?;
        let mut unique = std::collections::HashSet::with_capacity(capabilities.len());
        for capability in capabilities {
            let capability = capability
                .as_str()
                .ok_or_else(|| format!("grant entry for {key} contains a non-string capability"))?;
            if capability.trim().is_empty() {
                return Err(format!(
                    "grant entry for {key} contains an empty capability"
                ));
            }
            if !unique.insert(capability) {
                return Err(format!(
                    "grant entry for {key} repeats capability {capability}"
                ));
            }
        }
        if let Some(alias) = entry.get("display_alias")
            && !alias.is_string()
        {
            return Err(format!("display_alias for {key} must be a string"));
        }
        if key.starts_with('$') {
            if entry.contains_key("groups") {
                return Err(format!("group {key} cannot belong to another group"));
            }
            continue;
        }
        if key.is_empty() {
            return Err("grant document contains an empty principal_id".to_string());
        }
        if let Some(groups) = entry.get("groups") {
            let groups = groups
                .as_array()
                .ok_or_else(|| format!("groups for {key} must be an array"))?;
            let mut seen = std::collections::HashSet::with_capacity(groups.len());
            for group in groups {
                let group = group
                    .as_str()
                    .ok_or_else(|| format!("groups for {key} contains a non-string"))?;
                if !GRANT_GROUPS.contains(&group) || !entries.contains_key(group) {
                    return Err(format!("principal {key} references unknown group {group}"));
                }
                if !seen.insert(group) {
                    return Err(format!("principal {key} repeats group {group}"));
                }
            }
        }
    }
    Ok(entries)
}

/// Parse one principal's grants from a document of principals and groups.
///
/// The complete document must pass the same validation used by the grant
/// materializer. Looking up a group name returns nothing: groups are not callers.
pub fn capabilities_for_principal(
    document: &serde_json::Value,
    principal_id: &str,
) -> std::collections::HashSet<String> {
    if principal_id.is_empty() || principal_id == "*" || principal_id.starts_with('$') {
        return std::collections::HashSet::new();
    }
    let Ok(entries) = validated_grant_entries(document) else {
        return std::collections::HashSet::new();
    };
    let Some(entry) = entries.get(principal_id) else {
        return std::collections::HashSet::new();
    };
    let mut granted = std::collections::HashSet::new();
    let Some(own) = entry
        .get("capabilities")
        .and_then(serde_json::Value::as_array)
    else {
        return std::collections::HashSet::new();
    };
    granted.extend(
        own.iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_string),
    );
    let Some(groups) = entry.get("groups") else {
        return granted;
    };
    let Some(groups) = groups.as_array() else {
        return std::collections::HashSet::new();
    };
    for group in groups {
        let Some(name) = group.as_str() else {
            return std::collections::HashSet::new();
        };
        if !GRANT_GROUPS.contains(&name) {
            return std::collections::HashSet::new();
        }
        let Some(capabilities) = entries
            .get(name)
            .and_then(|group_entry| group_entry.get("capabilities"))
            .and_then(serde_json::Value::as_array)
        else {
            return std::collections::HashSet::new();
        };
        granted.extend(
            capabilities
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string),
        );
    }
    granted
}

#[cfg(test)]
mod tests {
    use super::{capabilities_for_principal, validate_grant_document};

    #[test]
    fn accepts_exact_principals_and_declared_groups() {
        assert!(validate_grant_document(
            br#"{"$GUEST":{"capabilities":["read"]},"person":{"groups":["$GUEST"],"capabilities":["write"]}}"#
        )
        .is_ok());
    }

    #[test]
    fn rejects_prohibited_keys_and_missing_groups() {
        for invalid in [
            br#"{"*":{"capabilities":[]}}"#.as_slice(),
            br#"{"$UNKNOWN":{"capabilities":[]}}"#.as_slice(),
            br#"{"person":{"groups":["$GUEST"],"capabilities":[]}}"#.as_slice(),
            br#"{"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef":{"capabilities":[]}}"#.as_slice(),
        ] {
            assert!(validate_grant_document(invalid).is_err());
        }
    }

    #[test]
    fn grants_only_exact_principals_from_a_valid_complete_document() {
        let mut document = serde_json::json!({
            "$GUEST": {"capabilities": ["read"]},
            "person": {"groups": ["$GUEST"], "capabilities": ["write"]}
        });
        let grants = capabilities_for_principal(&document, "person");
        assert_eq!(grants.len(), 2);
        assert!(grants.contains("read") && grants.contains("write"));
        assert!(capabilities_for_principal(&document, "$GUEST").is_empty());
        assert!(capabilities_for_principal(&document, "other").is_empty());
        document["*"] = serde_json::json!({"capabilities": ["admin"]});
        assert!(capabilities_for_principal(&document, "person").is_empty());
    }
}
