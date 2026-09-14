use std::{fs, path::Path};

fn rust_files(root: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(root).expect("read source tree") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn grep_gates() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let mut files = Vec::new();
    rust_files(&workspace.join("crates"), &mut files);

    let forbidden = [
        "write_sled_from_wg",
        "write_sled_full",
        "SENTINEL_FOOTPRINT",
        "anna_scribe",
        "plugin_schema.dat",
        "OP_SLED_PATH",
        "SHM_SLED_PATH",
        "op-identity-sled",
        "op-sled-top",
    ];
    let mut failures = Vec::new();

    for path in files {
        let source = fs::read_to_string(&path).expect("read Rust source");
        let relative = path.strip_prefix(workspace).unwrap_or(&path);
        if relative.ends_with("crates/op-grpc-bridge/tests/session_genesis_grep_gates.rs") {
            continue;
        }
        for (index, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for pattern in forbidden {
                if code.contains(pattern) {
                    failures.push(format!(
                        "{}:{}: forbidden {pattern}",
                        relative.display(),
                        index + 1
                    ));
                }
            }
            if code.contains("etch_footprint") {
                failures.push(format!(
                    "{}:{}: forbidden etch_footprint",
                    relative.display(),
                    index + 1
                ));
            }
            if relative.starts_with("crates/op-grpc-bridge")
                && code.contains("tracing::")
                && code.to_ascii_lowercase().contains("genesis")
                && !relative.starts_with("crates/op-grpc-bridge/tests")
            {
                failures.push(format!(
                    "{}:{}: genesis referenced by tracing macro",
                    relative.display(),
                    index + 1
                ));
            }
        }

        if source.contains("pub fn mint_genesis")
            && !relative.ends_with("crates/op-identity/src/session_genesis.rs")
        {
            failures.push(format!(
                "{}: mint_genesis implementation outside session_genesis.rs",
                relative.display()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "session-genesis grep gates failed:\n{}",
        failures.join("\n")
    );
}
