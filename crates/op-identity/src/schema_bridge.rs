//! Session-genesis verification and sealed schema-catalog identity.
//!
//! Identity is resolved from the per-session `identity_sled` projection.
//! Plugin schemas and their catalog hash are resolved from the sealed blob
//! catalog. This module intentionally has no process-wide identity fallback.

/// Seven operational categories for the subid taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubidCategory {
    Src,
    Prj,
    Sch,
    Mut,
    Obs,
    Evt,
    Exp,
}

impl SubidCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Src => "src",
            Self::Prj => "prj",
            Self::Sch => "sch",
            Self::Mut => "mut",
            Self::Obs => "obs",
            Self::Evt => "evt",
            Self::Exp => "exp",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "src" => Some(Self::Src),
            "prj" => Some(Self::Prj),
            "sch" => Some(Self::Sch),
            "mut" => Some(Self::Mut),
            "obs" => Some(Self::Obs),
            "evt" => Some(Self::Evt),
            "exp" => Some(Self::Exp),
            _ => None,
        }
    }
}

/// Parsed OSCAL subid components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubidTaxonomy {
    pub category: SubidCategory,
    pub component_type: String,
    pub subject: String,
    pub verb: String,
    pub facet: Option<String>,
    pub version: u8,
}

impl SubidTaxonomy {
    pub fn parse(s: &str) -> Result<Self, String> {
        let (body, version) = if let Some(at) = s.rfind('@') {
            let ver_str = &s[at + 1..];
            let version = ver_str
                .strip_prefix('v')
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| format!("invalid version suffix: {ver_str}"))?;
            (&s[..at], version)
        } else {
            (s, 0)
        };

        let mut parts = body.splitn(5, '.');
        let category_name = parts.next().ok_or("missing category")?;
        let category = SubidCategory::parse(category_name)
            .ok_or_else(|| format!("unknown category: {category_name}"))?;
        let component_type = parts.next().ok_or("missing component-type")?.to_string();
        let subject = parts.next().ok_or("missing subject")?.to_string();
        let verb = parts.next().ok_or("missing verb")?.to_string();
        let facet = parts.next().map(str::to_string);

        for segment in [&component_type, &subject, &verb] {
            if segment.is_empty()
                || !segment
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit())
            {
                return Err(format!(
                    "invalid segment '{segment}': must be lowercase ascii/digits/hyphens"
                ));
            }
        }

        Ok(Self {
            category,
            component_type,
            subject,
            verb,
            facet,
            version,
        })
    }

    pub fn canonical(&self) -> String {
        let mut value = format!(
            "{}.{}.{}.{}",
            self.category.as_str(),
            self.component_type,
            self.subject,
            self.verb,
        );
        if let Some(facet) = &self.facet {
            value.push('.');
            value.push_str(facet);
        }
        if self.version > 0 {
            value.push_str(&format!("@v{}", self.version));
        }
        value
    }
}

impl std::fmt::Display for SubidTaxonomy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.canonical())
    }
}

/// Why a request genesis failed to verify against a projected session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionGenesisVerifyError {
    /// The identity projection could not be read or decoded.
    SledUnreachable,
    /// The selected session exists but has not minted an anchored genesis.
    InvalidSled,
    /// The selected session was torn down or revoked. Parked (Incus stopped)
    /// is not this error — `active` is power policy, not identity.
    Inactive,
    /// The selected session term has elapsed.
    Expired,
    /// The session selector or supplied genesis did not match.
    Mismatch,
}

/// Verify a presented genesis against exactly one projected session record.
pub fn resolve_verified_session(
    request_genesis: &str,
    trace_id: Option<&str>,
    wireguard_pubkey: Option<&str>,
) -> Result<crate::session_projection::SessionIdentity, SessionGenesisVerifyError> {
    let sessions = crate::session_projection::read_identity_sessions()
        .map_err(|_| SessionGenesisVerifyError::SledUnreachable)?;
    let trace_id = trace_id.map(str::trim).filter(|value| !value.is_empty());
    let wireguard_pubkey = wireguard_pubkey
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let request_genesis = request_genesis.trim();

    let mut matching = sessions.into_iter().filter(|session| {
        if trace_id.is_some() || wireguard_pubkey.is_some() {
            trace_id.is_none_or(|value| session.trace_id == value)
                && wireguard_pubkey.is_none_or(|value| session.wireguard_pubkey == value)
        } else {
            session.genesis.as_deref() == Some(request_genesis)
        }
    });

    let session = matching.next().ok_or(SessionGenesisVerifyError::Mismatch)?;
    if matching.next().is_some() {
        return Err(SessionGenesisVerifyError::Mismatch);
    }
    if !session.is_anchored() {
        return Err(SessionGenesisVerifyError::InvalidSled);
    }
    if !session.is_current() {
        return Err(SessionGenesisVerifyError::Expired);
    }
    if session.genesis.as_deref() != Some(request_genesis) {
        return Err(SessionGenesisVerifyError::Mismatch);
    }
    Ok(session)
}

/// Verify a presented genesis against exactly one current projected session.
pub fn verify_session_genesis(
    request_genesis: &str,
    trace_id: Option<&str>,
    wireguard_pubkey: Option<&str>,
) -> Result<(), SessionGenesisVerifyError> {
    resolve_verified_session(request_genesis, trace_id, wireguard_pubkey).map(|_| ())
}

const SEALED_BLOB_MANIFEST_PATH: &str = "/dev/shm/opdbus/plugin-blobs/.manifest.json";

fn manifest_hash(field: &str) -> Option<[u8; 32]> {
    let bytes = std::fs::read(SEALED_BLOB_MANIFEST_PATH).ok()?;
    let manifest: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let raw = hex::decode(manifest.get(field)?.as_str()?).ok()?;
    <[u8; 32]>::try_from(raw.as_slice()).ok()
}

/// Previous sealed catalog hash retained for the bounded reseal window.
pub fn schema_catalog_hash_previous() -> Option<[u8; 32]> {
    manifest_hash("previous_catalog_hash")
}

/// Canonical catalog hash published once by the sealed blob catalog.
pub fn schema_catalog_hash() -> Option<[u8; 32]> {
    manifest_hash("catalog_hash")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_canonicalizes_subid() {
        let parsed = SubidTaxonomy::parse("sch.network.plugin-schema.resolve@v1").unwrap();
        assert_eq!(parsed.category, SubidCategory::Sch);
        assert_eq!(parsed.canonical(), "sch.network.plugin-schema.resolve@v1");
    }
}
