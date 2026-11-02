use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn materializer_publishes_only_valid_grants_and_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.json");
    let target = dir.path().join("runtime/grants.json");
    let ready = dir.path().join("run/ready");
    let run = |command: &str| {
        Command::new(env!("CARGO_BIN_EXE_op-grants-materializer"))
            .arg(command)
            .env("OP_GRANTS_SOURCE", &source)
            .env("OP_GRANTS_PATH", &target)
            .env("OP_GRANTS_READY", &ready)
            .output()
            .unwrap()
    };

    let valid = br#"{"$GUEST":{"capabilities":["cap.example.read@v1"]},"principal":{"groups":["$GUEST"],"capabilities":[]}}"#;
    fs::write(&source, valid).unwrap();
    assert!(run("once").status.success());
    assert_eq!(fs::read(&target).unwrap(), valid);
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(ready.exists());
    assert!(run("check").status.success());

    fs::write(&source, br#"{"*":{"capabilities":["admin"]}}"#).unwrap();
    assert!(!run("once").status.success());
    assert_eq!(fs::read(&target).unwrap(), b"{}\n");
    assert!(!ready.exists());

    for invalid in [
        br#"{"$UNKNOWN":{"capabilities":[]}}"#.as_slice(),
        br#"{"principal":{"groups":["$GUEST"],"capabilities":[]}}"#.as_slice(),
        br#"{"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef":{"capabilities":[]}}"#.as_slice(),
    ] {
        fs::write(&source, invalid).unwrap();
        assert!(!run("validate").status.success());
    }
}
