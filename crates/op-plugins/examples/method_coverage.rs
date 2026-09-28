//! Offline acceptance evidence for every method in a manifest-pinned catalog.
//! This report is never a runtime schema, registry, or plugin existence source.
//! `inventory <catalog>` emits the full denominator. `check <catalog> <claims>`
//! rejects missing methods, duplicate claims, stale contracts and unexecuted tests.
use anyhow::{Context, Result, bail, ensure};
use op_blob::BlobRef;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
struct Method {
    plugin: String,
    method: String,
    schema_hash: String,
}

#[derive(Debug, Deserialize)]
struct Claim {
    plugin: String,
    method: String,
    schema_hash: String,
    positive_tests: Vec<Proof>,
}

#[derive(Debug, Deserialize)]
struct Proof {
    /// Exact libtest name, including its module path; not a substring filter.
    test: String,
    /// Human-reviewed description of the positive behavior asserted by this test.
    behavior: String,
    scope: String,
    /// Test source must still match the source associated with the execution.
    source: PathBuf,
    source_sha256: String,
    /// Captured libtest stdout/stderr from an actually executed test run.
    log: PathBuf,
    log_sha256: String,
    exit_code: i32,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn inventory(dir: &Path) -> Result<Vec<Method>> {
    let manifest_path = dir.join(".manifest.json");
    let before = std::fs::read(&manifest_path)?;
    let manifest: serde_json::Value = serde_json::from_slice(&before)?;
    let plugins: BTreeMap<String, String> = serde_json::from_value(manifest["plugins"].clone())?;
    ensure!(
        !plugins.is_empty(),
        "empty catalog cannot establish coverage"
    );
    let mut methods = Vec::new();
    for (plugin, schema_hash) in plugins {
        ensure!(
            !plugin.is_empty()
                && plugin
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && schema_hash.len() == 64
                && schema_hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid catalog member"
        );
        let bytes = std::fs::read(dir.join(format!("{plugin}.{}.blob", &schema_hash[..16])))?;
        let blob =
            BlobRef::from_manifest_pinned(&bytes, &schema_hash).map_err(anyhow::Error::msg)?;
        let schema = blob.state_store_schema().map_err(anyhow::Error::msg)?;
        ensure!(schema.name == plugin, "catalog member identity mismatch");
        for method in schema.methods.keys() {
            methods.push(Method {
                plugin: plugin.clone(),
                method: method.clone(),
                schema_hash: schema_hash.clone(),
            });
        }
    }
    ensure!(
        before == std::fs::read(manifest_path)?,
        "catalog changed during inventory"
    );
    methods.sort_by(|a, b| (&a.plugin, &a.method).cmp(&(&b.plugin, &b.method)));
    ensure!(!methods.is_empty(), "catalog has no declared methods");
    Ok(methods)
}

fn passed_exactly(log: &str, test: &str) -> bool {
    let expected = format!("test {test} ... ok");
    log.lines().any(|line| line.trim() == expected)
}

fn validate_proof(proof: &Proof) -> Result<()> {
    ensure!(proof.exit_code == 0, "test process failed");
    ensure!(
        !proof.test.trim().is_empty() && !proof.behavior.trim().is_empty(),
        "positive behavior and exact test name are required"
    );
    ensure!(
        matches!(proof.scope.as_str(), "owner" | "canonical" | "native"),
        "proof scope must identify owner, canonical, or native behavior"
    );
    let source = std::fs::read(&proof.source)
        .with_context(|| format!("read test source {}", proof.source.display()))?;
    ensure!(
        digest(&source) == proof.source_sha256,
        "test source changed"
    );
    let log = std::fs::read(&proof.log)
        .with_context(|| format!("read execution log {}", proof.log.display()))?;
    ensure!(digest(&log) == proof.log_sha256, "execution log changed");
    ensure!(
        passed_exactly(std::str::from_utf8(&log)?, &proof.test),
        "exact positive test did not execute successfully"
    );
    Ok(())
}

fn check(methods: &[Method], claims: &[Claim]) -> Result<serde_json::Value> {
    let expected: BTreeMap<_, _> = methods
        .iter()
        .map(|m| {
            (
                (m.plugin.as_str(), m.method.as_str()),
                m.schema_hash.as_str(),
            )
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut failures = Vec::new();
    let mut passed = 0;
    for claim in claims {
        let key = (claim.plugin.as_str(), claim.method.as_str());
        let label = format!("{}.{}", key.0, key.1);
        if !seen.insert(key) {
            failures.push(format!("{label}: duplicate claim"));
            continue;
        }
        let Some(hash) = expected.get(&key) else {
            failures.push(format!("{label}: not declared in this catalog"));
            continue;
        };
        if *hash != claim.schema_hash {
            failures.push(format!("{label}: schema changed since the claim"));
            continue;
        }
        if claim.positive_tests.is_empty() {
            failures.push(format!(
                "{label}: no positive behavioral execution evidence"
            ));
            continue;
        }
        let errors: Vec<_> = claim
            .positive_tests
            .iter()
            .filter_map(|proof| validate_proof(proof).err())
            .collect();
        if errors.is_empty() {
            passed += 1;
        } else {
            for error in errors {
                failures.push(format!("{label}: {error:#}"));
            }
        }
    }
    for key in expected.keys().filter(|key| !seen.contains(*key)) {
        failures.push(format!("{}.{}: missing claim", key.0, key.1));
    }
    Ok(serde_json::json!({
        "purpose": "offline behavioral evidence; never runtime schema authority",
        "complete": failures.is_empty(),
        "declared_methods": methods.len(),
        "methods_with_execution_evidence": passed,
        "failures": failures,
        "limits": "Evidence descriptions require review of actual test assertions. The full workspace/source-admission gate must also pass against the unchanged implementation. Owner fixtures do not prove canonical transport or live provider integration."
    }))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [mode, dir] if mode == "inventory" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&inventory(Path::new(dir))?)?
            );
        }
        [mode, dir, claims] if mode == "check" => {
            let methods = inventory(Path::new(dir))?;
            let claims: Vec<Claim> = serde_json::from_slice(&std::fs::read(claims)?)?;
            let report = check(&methods, &claims)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            ensure!(report["complete"] == true, "method coverage is incomplete");
        }
        _ => bail!("usage: method_coverage inventory <catalog> | check <catalog> <claims.json>"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_filtered_and_similarly_named_tests_are_not_execution() {
        for text in [
            "test owner::method ... ignored",
            "test owner::method_extra ... ok",
            "test owner::method ... FAILED",
            "test result: ok. 0 passed; 1 filtered out",
        ] {
            assert!(!passed_exactly(text, "owner::method"));
        }
        assert!(passed_exactly(
            "test owner::method ... ok\n",
            "owner::method"
        ));
    }

    #[test]
    fn missing_duplicate_unknown_and_contract_drift_fail_completion() {
        let methods = vec![Method {
            plugin: "plugin".into(),
            method: "method".into(),
            schema_hash: "hash".into(),
        }];
        assert_eq!(check(&methods, &[]).unwrap()["complete"], false);
        let claim = || Claim {
            plugin: "plugin".into(),
            method: "method".into(),
            schema_hash: "stale".into(),
            positive_tests: vec![],
        };
        let mut unknown = claim();
        unknown.method = "undeclared".into();
        let report = check(&methods, &[claim(), claim(), unknown]).unwrap();
        let failures = report["failures"].as_array().unwrap();
        assert_eq!(failures.len(), 3);
        assert_eq!(report["methods_with_execution_evidence"], 0);
    }

    #[test]
    fn changed_source_logs_and_failed_runs_cannot_reuse_a_passing_receipt() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("case.rs");
        let log = dir.path().join("case.log");
        std::fs::write(&source, b"test source at execution").unwrap();
        std::fs::write(&log, b"test owner::readback ... ok\n").unwrap();
        let mut proof = Proof {
            test: "owner::readback".into(),
            behavior: "asserts a committed value is read back".into(),
            scope: "owner".into(),
            source: source.clone(),
            source_sha256: digest(&std::fs::read(&source).unwrap()),
            log: log.clone(),
            log_sha256: digest(&std::fs::read(&log).unwrap()),
            exit_code: 0,
        };
        validate_proof(&proof).unwrap();
        proof.exit_code = 1;
        assert!(validate_proof(&proof).is_err());
        proof.exit_code = 0;
        std::fs::write(&source, b"changed after execution").unwrap();
        assert!(validate_proof(&proof).is_err());
        std::fs::write(&source, b"test source at execution").unwrap();
        std::fs::write(&log, b"test owner::readback ... ignored\n").unwrap();
        assert!(validate_proof(&proof).is_err());
        proof.log_sha256 = digest(&std::fs::read(&log).unwrap());
        assert!(validate_proof(&proof).is_err());
    }
}
