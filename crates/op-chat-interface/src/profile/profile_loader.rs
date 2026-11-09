//! Profile_Loader (spec task 4.1, D-22): size check before decoding,
//! strict UTF-8, duplicate-key rejection, JSON Schema validation,
//! Secret_Guard pass on every string, immutable result. Errors name the
//! field path and rule, never the value.

use crate::{
    guard::{GuardVerdict, SecretGuard},
    types::IdentityProfile,
};
use std::fmt;

pub const MAX_PROFILE_BYTES: usize = 65_536;
const PROFILE_SCHEMA: &str = include_str!("../../config/identity-profile.schema.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileError {
    pub path: String,
    pub rule: String,
}

impl ProfileError {
    fn new(path: impl Into<String>, rule: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            rule: rule.into(),
        }
    }
}
impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.rule)
    }
}
impl std::error::Error for ProfileError {}

pub fn parse(bytes: &[u8], guard: &SecretGuard) -> Result<IdentityProfile, ProfileError> {
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(ProfileError::new("", "maxBytes 65536"));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ProfileError::new("", "strict UTF-8"))?;
    reject_duplicate_keys(text)?;
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| ProfileError::new("", "valid JSON"))?;
    validate_schema(&value)?;
    validate_fields(&value)?;
    for string in all_strings(&value) {
        if !guard.check_item(string.as_bytes()).is_clean() {
            return Err(ProfileError::new("", "credential-refused"));
        }
    }
    serde_json::from_value::<IdentityProfile>(value)
        .map_err(|_| ProfileError::new("", "profile shape"))
}

/// Duplicates are rejected at any depth using serde's streaming visitor,
/// which sees every key even though a map would collapse them.
fn reject_duplicate_keys(text: &str) -> Result<(), ProfileError> {
    fn visit(value: &serde_json::Value) -> Result<(), ProfileError> {
        match value {
            serde_json::Value::Object(map) => map.values().try_for_each(visit),
            serde_json::Value::Array(items) => items.iter().try_for_each(visit),
            _ => Ok(()),
        }
    }
    // First pass on the collapsed value proves the text parses; duplicate
    // keys are then found by scanning raw object key tokens at each depth.
    let parsed: serde_json::Value =
        serde_json::from_str(text).map_err(|_| ProfileError::new("", "valid JSON"))?;
    visit(&parsed)?;
    scan_duplicates(text)
}

fn scan_duplicates(text: &str) -> Result<(), ProfileError> {
    // Proper frame tracking: strings are keys only in the expect-key
    // state of an object frame; array elements are values, never keys.
    #[derive(Clone, Copy, PartialEq)]
    enum Ctx {
        ObjectExpectKey,
        ObjectExpectValue,
        Array,
    }
    let bytes = text.as_bytes();
    let mut stack: Vec<(Ctx, std::collections::HashSet<String>)> = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    let mut escaped = false;
    let mut string_start = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
                if matches!(stack.last().map(|f| f.0), Some(Ctx::ObjectExpectKey)) {
                    let key: String =
                        serde_json::from_str(&text[string_start..=i]).unwrap_or_default();
                    let frame = stack.last_mut().expect("object frame");
                    if !frame.1.insert(key) {
                        return Err(ProfileError::new("", "duplicate key"));
                    }
                    frame.0 = Ctx::ObjectExpectValue;
                }
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                in_string = true;
                string_start = i;
            }
            b'{' => stack.push((Ctx::ObjectExpectKey, Default::default())),
            b'}' | b']' => {
                stack.pop();
            }
            b'[' => stack.push((Ctx::Array, Default::default())),
            b',' => {
                if let Some(frame) = stack.last_mut() {
                    if frame.0 == Ctx::ObjectExpectValue {
                        frame.0 = Ctx::ObjectExpectKey;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

fn validate_schema(value: &serde_json::Value) -> Result<(), ProfileError> {
    let schema: serde_json::Value = serde_json::from_str(PROFILE_SCHEMA)
        .map_err(|_| ProfileError::new("", "embedded schema"))?;
    let validator = jsonschema::validator_for(&schema)
        .map_err(|_| ProfileError::new("", "embedded schema"))?;
    if let Some(error) = validator.iter_errors(value).next() {
        return Err(ProfileError::new(
            error.instance_path.to_string(),
            error.kind.to_string(),
        ));
    }
    Ok(())
}

fn validate_fields(value: &serde_json::Value) -> Result<(), ProfileError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ProfileError::new("", "type object"))?;
    if let Some(name) = obj.get("display_name").and_then(|v| v.as_str()) {
        if name.chars().any(|c| c.is_control()) {
            return Err(ProfileError::new("/display_name", "no control characters"));
        }
    }
    if obj.get("session_mode").is_some_and(|v| !v.is_string()) {
        return Err(ProfileError::new("/session_mode", "type string"));
    }
    if let Some(targets) = obj.get("deployment_targets").and_then(|v| v.as_array()) {
        for (index, target) in targets.iter().enumerate() {
            let paths = target
                .get("mount_paths")
                .and_then(|v| v.as_object())
                .ok_or_else(|| {
                    ProfileError::new(format!("/deployment_targets/{index}/mount_paths"), "type object")
                })?;
            if paths.len() != 2 {
                return Err(ProfileError::new(
                    format!("/deployment_targets/{index}/mount_paths"),
                    "exactly 3tched and threetched keys",
                ));
            }
            for (key, path) in paths {
                let path_str = path.as_str().unwrap_or_default();
                if !path_str.starts_with('/') || path_str.split('/').any(|part| part == "..") {
                    return Err(ProfileError::new(
                        format!("/deployment_targets/{index}/mount_paths/{key}"),
                        "absolute path without ..",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn all_strings(value: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                out.extend(all_strings(item));
            }
        }
        serde_json::Value::Object(map) => {
            for (key, member) in map {
                out.push(key.clone());
                out.extend(all_strings(member));
            }
        }
        _ => {}
    }
    out
}

/// Canonical JSON in schema key order; list order preserved (task 4.1).
pub fn format(profile: &IdentityProfile) -> String {
    let mut out = String::from("{");
    let mut push = |out: &mut String, key: &str, value: serde_json::Value, first: bool| {
        if !first {
            out.push(',');
        }
        out.push_str(&serde_json::to_string(key).unwrap_or_default());
        out.push(':');
        out.push_str(&value.to_string());
    };
    push(&mut out, "format", serde_json::json!(profile.format), true);
    push(&mut out, "principal_id", serde_json::json!(profile.principal_id), false);
    push(&mut out, "display_name", serde_json::json!(profile.display_name), false);
    if let Some(v) = &profile.expected_session_id {
        push(&mut out, "expected_session_id", serde_json::json!(v), false);
    }
    if let Some(v) = &profile.session_mode {
        push(&mut out, "session_mode", serde_json::json!(v), false);
    }
    let targets: Vec<serde_json::Value> = profile
        .deployment_targets
        .iter()
        .map(|t| {
            serde_json::json!({
                "kind": t.kind,
                "mount_paths": t.mount_paths,
            })
        })
        .collect();
    push(&mut out, "deployment_targets", serde_json::Value::Array(targets), false);
    out.push('}');
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Standard,
    ControlPlane,
    UiBuild,
}

impl SessionMode {
    pub fn resolve(value: Option<&serde_json::Value>) -> Result<Self, ProfileError> {
        match value {
            None => Ok(Self::Standard),
            Some(serde_json::Value::String(s)) => Ok(match s.as_str() {
                "control_plane" => Self::ControlPlane,
                "ui_build" => Self::UiBuild,
                _ => Self::Standard,
            }),
            Some(_) => Err(ProfileError::new("/session_mode", "type string")),
        }
    }

    pub fn tool_call_limit(self) -> u32 {
        match self {
            Self::Standard => 5,
            Self::ControlPlane | Self::UiBuild => 5000,
        }
    }
}
