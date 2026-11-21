//! Path component encoding for 3tchedFS operation directories.
//!
//! Mirrors `_3tched_fs::namespace::encode_component` exactly (that module is
//! the authority; this copy exists because the door does not link 3tchedFS).

/// Encode one plugin or method name as a mount path component.
pub fn encode_component(value: &str) -> String {
    if value == ".value.json" {
        return "%2Evalue.json".to_owned();
    }
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    if output == "." || output == ".." || output.is_empty() {
        format!(
            "%{}",
            value
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        )
    } else {
        output
    }
}

/// The mount-relative operation directory for `plugin.method`.
pub fn operation_dir(plugin: &str, method: &str) -> String {
    format!(
        "plugins/{}/operations/{}",
        encode_component(plugin),
        encode_component(method)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_3tchedfs_encoding() {
        assert_eq!(encode_component("memory_recall"), "memory_recall");
        assert_eq!(encode_component("a.b-c_d"), "a.b-c_d");
        assert_eq!(encode_component("a/b"), "a%2Fb");
        assert_eq!(encode_component("a b"), "a%20b");
        assert_eq!(encode_component(".value.json"), "%2Evalue.json");
        assert_eq!(encode_component("."), "%2E");
        assert_eq!(encode_component(".."), "%2E2E");
        assert_eq!(encode_component(""), "%");
        assert_eq!(
            operation_dir("cognitive", "memory_recall"),
            "plugins/cognitive/operations/memory_recall"
        );
    }
}
