use super::*;
use std::{
    fs::File,
    path::Path,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex as StdMutex},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::net::UnixListener;

struct Fixture {
    _root: tempfile::TempDir,
    service: MailAdminService,
    _showq_task: tokio::task::JoinHandle<()>,
    _qmgr_task: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self._showq_task.abort();
        self._qmgr_task.abort();
    }
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("etc");
        let queue = root.path().join("queue");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&queue).unwrap();
        let paths = NativePaths {
            domains: config.join("domains"),
            mailboxes: config.join("vmailbox"),
            aliases: config.join("virtual"),
            dovecot_users: config.join("users"),
            dovecot_users_gid: None,
            mail_root: root.path().join("mail"),
            mail_uid: None,
            mail_gid: None,
            showq_socket: queue.join("showq"),
            qmgr_socket: queue.join("qmgr"),
            postfix_master_pid: None,
        };
        std::fs::write(
            &paths.mailboxes,
            "owner@example.com example.com/owner/Maildir/\n",
        )
        .unwrap();
        std::fs::write(
            &paths.dovecot_users,
            "owner@example.com:{SHA512-CRYPT}$6$fixture::::::\n",
        )
        .unwrap();
        std::fs::write(&paths.aliases, "").unwrap();

        let showq = UnixListener::bind(&paths.showq_socket).unwrap();
        let showq_task = tokio::spawn(async move {
            let response = b"protocol\0mail_queue_list_protocol\0\0queue_name\0deferred\0queue_id\0Q1\0time\x001700000000\0size\x001234\0forced_expire\x000\0sender\0sender@example.net\0original_recipient\0\0recipient\0owner@example.com\0log_class\0defer\0reason\0temporary failure\0\0\0";
            while let Ok((mut stream, _)) = showq.accept().await {
                let _ = stream.write_all(response).await;
                let _ = stream.shutdown().await;
            }
        });
        let qmgr = UnixListener::bind(&paths.qmgr_socket).unwrap();
        let qmgr_requests = Arc::new(StdMutex::new(Vec::new()));
        let requests = qmgr_requests.clone();
        let qmgr_task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = qmgr.accept().await {
                let mut request = Vec::new();
                let _ = stream.read_to_end(&mut request).await;
                requests.lock().unwrap().push(request);
            }
        });
        let service = MailAdminService::new(
            RuntimeDocuments::new(CozoGraphShuttle::new_in_memory().unwrap()),
            paths,
        );
        service.adopt_or_reconcile().await.unwrap();
        Self {
            _root: root,
            service,
            _showq_task: showq_task,
            _qmgr_task: qmgr_task,
        }
    }
}

#[tokio::test]
async fn mail_admin_interface_denies_all_mutations_without_verified_grant() {
    let root = tempfile::tempdir().unwrap();
    let paths = NativePaths {
        domains: root.path().join("domains"),
        mailboxes: root.path().join("vmailbox"),
        aliases: root.path().join("virtual"),
        dovecot_users: root.path().join("users"),
        dovecot_users_gid: None,
        mail_root: root.path().join("mail"),
        mail_uid: None,
        mail_gid: None,
        showq_socket: root.path().join("absent-showq"),
        qmgr_socket: root.path().join("absent-qmgr"),
        postfix_master_pid: None,
    };
    std::fs::write(&paths.domains, "example.com\tOK\n").unwrap();
    std::fs::write(
        &paths.mailboxes,
        "owner@example.com example.com/owner/Maildir/\n",
    )
    .unwrap();
    std::fs::write(
        &paths.dovecot_users,
        "owner@example.com:{SHA512-CRYPT}$6$fixture::::::\n",
    )
    .unwrap();
    std::fs::write(&paths.aliases, "").unwrap();
    let service = MailAdminService::new(
        RuntimeDocuments::new(CozoGraphShuttle::new_in_memory().unwrap()),
        paths,
    );
    service.adopt_or_reconcile().await.unwrap();
    let before = service
        .documents
        .read_durable(DOCUMENT_ID)
        .await
        .unwrap()
        .unwrap();
    let before_domains = std::fs::read(&service.paths.domains).unwrap();
    let before_mailboxes = std::fs::read(&service.paths.mailboxes).unwrap();
    let before_aliases = std::fs::read(&service.paths.aliases).unwrap();
    let before_users = std::fs::read(&service.paths.dovecot_users).unwrap();
    fn denied(result: zbus::fdo::Result<String>) {
        assert!(matches!(result, Err(zbus::fdo::Error::AccessDenied(_))));
    }
    denied(service.add_domain("example.net").await);
    denied(service.remove_domain("example.com").await);
    denied(
        service
            .add_mailbox("example.net", "alice", "{SHA512-CRYPT}$6$fixture")
            .await,
    );
    denied(service.remove_mailbox("example.com", "owner").await);
    denied(
        service
            .add_alias("example.net", "info", "alice@example.net")
            .await,
    );
    denied(service.remove_alias("example.com", "info").await);
    denied(service.set_quota("example.com", "owner", 256).await);
    denied(service.flush_queue().await);
    let after = service
        .documents
        .read_durable(DOCUMENT_ID)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.revision, before.revision);
    assert_eq!(after.document, before.document);
    assert_eq!(
        std::fs::read(&service.paths.domains).unwrap(),
        before_domains
    );
    assert_eq!(
        std::fs::read(&service.paths.mailboxes).unwrap(),
        before_mailboxes
    );
    assert_eq!(
        std::fs::read(&service.paths.aliases).unwrap(),
        before_aliases
    );
    assert_eq!(
        std::fs::read(&service.paths.dovecot_users).unwrap(),
        before_users
    );
    assert!(!service.paths.qmgr_socket.exists());
}

#[tokio::test]
async fn committed_state_renders_exact_postfix_and_dovecot_inputs_and_preserves_maildir() {
    let fixture = Fixture::new().await;
    fixture
        .service
        .add_domain_inner("Example.NET")
        .await
        .unwrap();
    fixture
        .service
        .add_mailbox_inner("example.net", "Alice", "{SHA512-CRYPT}$6$new")
        .await
        .unwrap();
    fixture
        .service
        .set_quota_inner("example.net", "alice", 128)
        .await
        .unwrap();
    fixture
        .service
        .add_alias_inner("example.net", "info", "alice@example.net")
        .await
        .unwrap();

    let paths = &fixture.service.paths;
    assert_eq!(
        std::fs::read_to_string(&paths.domains).unwrap(),
        "example.com\tOK\nexample.net\tOK\n"
    );
    assert!(std::fs::read_to_string(&paths.mailboxes)
        .unwrap()
        .contains("alice@example.net\texample.net/alice/Maildir/"));
    assert_eq!(
        std::fs::read_to_string(&paths.aliases).unwrap(),
        "info@example.net\talice@example.net\n"
    );
    let users = std::fs::read_to_string(&paths.dovecot_users).unwrap();
    assert!(users.contains(
        "alice@example.net:{SHA512-CRYPT}$6$new::::::userdb_quota_rule=*:bytes=134217728"
    ));
    let maildir = paths.mail_root.join("example.net/alice/Maildir");
    assert!(maildir.is_dir());

    fixture
        .service
        .remove_alias_inner("example.net", "info")
        .await
        .unwrap();
    let removed = fixture
        .service
        .remove_mailbox_inner("example.net", "alice")
        .await
        .unwrap();
    assert!(removed.maildir_preserved);
    assert!(maildir.is_dir());
    assert!(!std::fs::read_to_string(&paths.dovecot_users)
        .unwrap()
        .contains("alice@example.net"));
}

#[test]
fn showq_protocol_is_parsed_without_invoking_postqueue() {
    let wire = b"protocol\0mail_queue_list_protocol\0\0queue_name\0active\0queue_id\0ABC123\0time\x001700000000\0size\x0042\0forced_expire\x001\0sender\0sender@example.net\0original_recipient\0original@example.net\0recipient\0target@example.com\0log_class\0\0reason\0\0\0\0";
    let output = parse_showq(wire).unwrap();
    assert_eq!(output.summary.messages, 1);
    assert_eq!(output.summary.bytes, 42);
    assert_eq!(output.entries[0].queue_id, "ABC123");
    assert!(output.entries[0].forced_expire);
    assert_eq!(
        output.entries[0].recipients[0].address,
        "target@example.com"
    );
}

#[test]
fn validation_rejects_unsafe_config_tokens_and_plaintext_passwords() {
    assert!(validate_domain("example.com\nrelay_domains=*").is_err());
    assert!(validate_localpart("../owner").is_err());
    assert!(validate_password_hash("{PLAIN}secret").is_err());
    assert!(validate_address("one@example.com,two@example.com").is_err());
}

#[test]
fn explicit_mail_owner_must_match_prepared_root_before_config_effects() {
    use std::os::unix::fs::MetadataExt as _;

    let root = tempfile::tempdir().unwrap();
    let mail_root = root.path().join("mail");
    std::fs::create_dir(&mail_root).unwrap();
    let actual = std::fs::metadata(&mail_root).unwrap();
    let wrong_uid = if actual.uid() == 1 { 2 } else { 1 };
    let wrong_gid = if actual.gid() == 1 { 2 } else { 1 };
    let paths = NativePaths {
        domains: root.path().join("domains"),
        mailboxes: root.path().join("vmailbox"),
        aliases: root.path().join("virtual"),
        dovecot_users: root.path().join("users"),
        dovecot_users_gid: None,
        mail_root,
        mail_uid: Some(wrong_uid),
        mail_gid: Some(wrong_gid),
        showq_socket: root.path().join("showq"),
        qmgr_socket: root.path().join("qmgr"),
        postfix_master_pid: None,
    };

    let error = materialize(&paths, &MailDocument::default()).unwrap_err();
    assert!(error
        .to_string()
        .contains("prepared virtual mail root owner"));
    assert!(!paths.domains.exists());
    assert!(!paths.mailboxes.exists());
    assert!(!paths.dovecot_users.exists());
}

const NATIVE_MAIL_FIXTURE_ENV: &str = "OP_MAIL_NATIVE_CONTAINER_TEST";
const NATIVE_MAIL_CONTAINER: &str = "mail-3tched";

struct IsolatedContainerMailFixture {
    root: String,
    dovecot_config: String,
    dovecot_started: bool,
}

impl Drop for IsolatedContainerMailFixture {
    fn drop(&mut self) {
        if self.dovecot_started {
            let _ = Command::new("incus")
                .args([
                    "exec",
                    NATIVE_MAIL_CONTAINER,
                    "--",
                    "dovecot",
                    "-c",
                    &self.dovecot_config,
                    "stop",
                ])
                .output();
        }
        if self.root.starts_with("/tmp/opdbus-mail-native-") {
            let _ = Command::new("incus")
                .args([
                    "exec",
                    NATIVE_MAIL_CONTAINER,
                    "--",
                    "find",
                    &self.root,
                    "-depth",
                    "-delete",
                ])
                .output();
        }
    }
}

fn native_command(program: &str, args: &[&str]) -> Output {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("start {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} {args:?} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn incus_output(args: &[&str]) -> String {
    String::from_utf8(native_command("incus", args).stdout).expect("native output is UTF-8")
}

fn container_account_id(flag: &str, account: &str) -> String {
    let value = incus_output(&["exec", NATIVE_MAIL_CONTAINER, "--", "id", flag, account]);
    let value = value.trim();
    let id = value
        .parse::<u32>()
        .unwrap_or_else(|error| panic!("container {account} {flag} is not numeric: {error}"));
    assert_ne!(id, 0, "container {account} must not resolve to root");
    id.to_string()
}

fn push_fixture_file(source: &Path, destination: &str, gid: &str, mode: &str) {
    let output = Command::new("incus")
        .args([
            "exec",
            NATIVE_MAIL_CONTAINER,
            "--",
            "dd",
            &format!("of={destination}"),
            "status=none",
        ])
        .stdin(Stdio::from(
            File::open(source).expect("open generated fixture file"),
        ))
        .output()
        .expect("start incus file transfer");
    assert!(
        output.status.success(),
        "push {} failed: {}",
        source.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    native_command(
        "incus",
        &[
            "exec",
            NATIVE_MAIL_CONTAINER,
            "--",
            "chown",
            &format!("0:{gid}"),
            destination,
        ],
    );
    native_command(
        "incus",
        &[
            "exec",
            NATIVE_MAIL_CONTAINER,
            "--",
            "chmod",
            mode,
            destination,
        ],
    );
}

/// This fixture is opt-in because it needs the local, already-running Alpine
/// mail container. It creates only a uniquely named /tmp tree and a separate
/// Dovecot instance; it never reads or reloads the live mail configuration.
#[test]
fn owner_outputs_are_consumed_by_native_postfix_and_dovecot() {
    if std::env::var(NATIVE_MAIL_FIXTURE_ENV).as_deref() != Ok("1") {
        return;
    }
    native_command("incus", &["info", NATIVE_MAIL_CONTAINER]);
    let vmail_uid = container_account_id("-u", "vmail");
    let vmail_gid = container_account_id("-g", "vmail");
    let dovecot_gid = container_account_id("-g", "dovecot");

    let generated = tempfile::tempdir().unwrap();
    let paths = NativePaths {
        domains: generated.path().join("domains"),
        mailboxes: generated.path().join("vmailbox"),
        aliases: generated.path().join("virtual"),
        dovecot_users: generated.path().join("users"),
        dovecot_users_gid: None,
        mail_root: generated.path().join("mail"),
        mail_uid: None,
        mail_gid: None,
        showq_socket: generated.path().join("unused-showq"),
        qmgr_socket: generated.path().join("unused-qmgr"),
        postfix_master_pid: None,
    };
    let address = "fixture@example.test";
    let password_hash = "{SHA512-CRYPT}$6$bjUQ.GJHELfP5/oz$y74xYdNrpU/WYm4YBWv02em.u62TgwovhMV4WsYvAFBjWfkDVCgTHlqJcHUatXZXHwhkG2FFFb2n6dABXId2f.";
    let mut document = MailDocument::default();
    document.domains.insert("example.test".to_owned());
    document.mailboxes.insert(
        address.to_owned(),
        StoredMailbox {
            domain: "example.test".to_owned(),
            user: "fixture".to_owned(),
            password_hash: password_hash.to_owned(),
            quota_bytes: Some(64 * 1024 * 1024),
        },
    );
    document
        .aliases
        .insert("info@example.test".to_owned(), address.to_owned());
    materialize(&paths, &document).unwrap();

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = format!("/tmp/opdbus-mail-native-{}-{nonce}", std::process::id());
    let run = format!("{root}/run");
    let state = format!("{root}/state");
    let maildir = format!("{root}/mail/example.test/fixture/Maildir");
    let config = format!("{root}/dovecot.conf");
    let mut isolated = IsolatedContainerMailFixture {
        root: root.clone(),
        dovecot_config: config.clone(),
        dovecot_started: false,
    };
    native_command(
        "incus",
        &[
            "exec",
            NATIVE_MAIL_CONTAINER,
            "--",
            "install",
            "-d",
            "-m",
            "0777",
            &root,
            &run,
            &state,
            &format!("{maildir}/cur"),
            &format!("{maildir}/new"),
            &format!("{maildir}/tmp"),
        ],
    );
    native_command(
        "incus",
        &[
            "exec",
            NATIVE_MAIL_CONTAINER,
            "--",
            "chown",
            "-R",
            &format!("{vmail_uid}:{vmail_gid}"),
            &format!("{root}/mail"),
        ],
    );
    native_command(
        "incus",
        &[
            "exec",
            NATIVE_MAIL_CONTAINER,
            "--",
            "chmod",
            "-R",
            "0700",
            &format!("{root}/mail"),
        ],
    );
    for (source, name) in [
        (&paths.domains, "domains"),
        (&paths.mailboxes, "vmailbox"),
        (&paths.aliases, "virtual"),
        (&paths.dovecot_users, "users"),
    ] {
        let (gid, mode) = if name == "users" {
            (dovecot_gid.as_str(), "0640")
        } else {
            ("0", "0644")
        };
        push_fixture_file(source, &format!("{root}/{name}"), gid, mode);
    }

    let postfix_domain = incus_output(&[
        "exec",
        NATIVE_MAIL_CONTAINER,
        "--",
        "postmap",
        "-q",
        "example.test",
        &format!("texthash:{root}/domains"),
    ]);
    assert_eq!(postfix_domain.trim(), "OK");
    let postfix_mailbox = incus_output(&[
        "exec",
        NATIVE_MAIL_CONTAINER,
        "--",
        "postmap",
        "-q",
        address,
        &format!("texthash:{root}/vmailbox"),
    ]);
    assert_eq!(postfix_mailbox.trim(), "example.test/fixture/Maildir/");
    let postfix_alias = incus_output(&[
        "exec",
        NATIVE_MAIL_CONTAINER,
        "--",
        "postmap",
        "-q",
        "info@example.test",
        &format!("texthash:{root}/virtual"),
    ]);
    assert_eq!(postfix_alias.trim(), address);

    let dovecot_config = format!(
        r#"base_dir = {run}
state_dir = {state}
instance_name = opdbus-native-fixture-{nonce}
protocols =
listen = 127.0.0.1
ssl = no
disable_plaintext_auth = no
auth_mechanisms = plain login
auth_failure_delay = 0
mail_location = maildir:{root}/mail/%d/%n/Maildir
first_valid_uid = 100
log_path = {root}/dovecot.log
info_log_path = {root}/dovecot.log
passdb {{
  driver = passwd-file
  args = scheme=CRYPT username_format=%u {root}/users
}}
userdb {{
  driver = passwd-file
  args = username_format=%u {root}/users
  default_fields = uid={vmail_uid} gid={vmail_gid} home={root}/mail/%d/%n
}}
mail_plugins = quota
protocol imap {{
  mail_plugins = $mail_plugins imap_quota
}}
plugin {{
  quota = maildir:User quota
}}
service auth {{
  unix_listener auth-client {{
    mode = 0666
  }}
  unix_listener auth-userdb {{
    mode = 0666
  }}
}}
"#
    );
    let host_config = generated.path().join("dovecot.conf");
    std::fs::write(&host_config, dovecot_config).unwrap();
    push_fixture_file(&host_config, &config, "0", "0600");

    let parsed = incus_output(&[
        "exec",
        NATIVE_MAIL_CONTAINER,
        "--",
        "doveconf",
        "-c",
        &config,
        "-n",
    ]);
    assert!(parsed.contains("mail_plugins = quota"));
    // doveconf expands the inherited value; assert what native IMAP consumes.
    assert!(parsed.contains("mail_plugins = quota imap_quota"));
    native_command(
        "incus",
        &[
            "exec",
            NATIVE_MAIL_CONTAINER,
            "--",
            "dovecot",
            "-c",
            &config,
        ],
    );
    isolated.dovecot_started = true;

    let auth = incus_output(&[
        "exec",
        NATIVE_MAIL_CONTAINER,
        "--",
        "doveadm",
        "auth",
        "test",
        "-a",
        &format!("{run}/auth-client"),
        "-x",
        "service=imap",
        address,
        "fixture-password",
    ]);
    assert!(auth.contains("auth succeeded"));
    let user = incus_output(&[
        "exec",
        NATIVE_MAIL_CONTAINER,
        "--",
        "doveadm",
        "user",
        "-a",
        &format!("{run}/auth-userdb"),
        address,
    ]);
    assert!(user.contains("quota_rule\t*:bytes=67108864"));
    let quota = incus_output(&[
        "exec",
        NATIVE_MAIL_CONTAINER,
        "--",
        "doveadm",
        "-c",
        &config,
        "quota",
        "get",
        "-u",
        address,
    ]);
    assert!(quota.lines().any(|line| {
        line.contains("User quota") && line.contains("STORAGE") && line.contains("65536")
    }));
}
