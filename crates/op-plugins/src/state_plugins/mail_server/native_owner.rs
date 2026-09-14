//! Native Postfix/Dovecot administration owner for `mail-3tched`.
//!
//! The daemon using this module runs inside the mail container. Its private
//! peer-to-peer D-Bus socket is mounted into the host's ghostbridge directory.
//! Cozo is the durable authority; Postfix/Dovecot files are derived artifacts
//! replaced atomically after each committed transition.

use super::{
    AliasMutationOutput, DomainMutationOutput, FlushQueueOutput, GetQueueOutput,
    MailAdministrationState, MailAlias, MailMailbox, MailQueueEntry, MailQueueRecipient,
    MailQueueSummary, MailboxMutationOutput, QuotaMutationOutput,
};
use anyhow::{ensure, Context, Result};
use op_cozo_store::CozoGraphShuttle;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::Mutex,
};

use crate::state_plugins::runtime_documents::RuntimeDocuments;

const DOCUMENT_ID: &str = "mail_server_native";
const MAX_CONFIG_BYTES: usize = 4 * 1024 * 1024;
const MAX_QUEUE_BYTES: usize = 16 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct NativePaths {
    pub domains: PathBuf,
    pub mailboxes: PathBuf,
    pub aliases: PathBuf,
    pub dovecot_users: PathBuf,
    /// Numeric group allowed to read the password database. Offline image
    /// preparation resolves this from the target's Dovecot account.
    pub dovecot_users_gid: Option<u32>,
    pub mail_root: PathBuf,
    /// Prepared-image numeric owner for the virtual mail tree. Production
    /// requires both IDs; isolated fixtures may omit both and inherit their
    /// already-created temporary root owner.
    pub mail_uid: Option<u32>,
    pub mail_gid: Option<u32>,
    pub showq_socket: PathBuf,
    pub qmgr_socket: PathBuf,
    pub postfix_master_pid: Option<PathBuf>,
}

impl NativePaths {
    pub fn production() -> Result<Self> {
        let dovecot_users_gid = required_numeric_id("OP_MAIL_DOVECOT_GID")?;
        let mail_uid = required_numeric_id("OP_MAIL_VMAIL_UID")?;
        let mail_gid = required_numeric_id("OP_MAIL_VMAIL_GID")?;
        ensure!(
            dovecot_users_gid != 0,
            "Dovecot password reader group must not be root"
        );
        ensure!(mail_uid != 0, "virtual mail owner must not be root");
        ensure!(mail_gid != 0, "virtual mail group must not be root");
        let paths = Self {
            domains: "/etc/postfix/opdbus-domains".into(),
            mailboxes: "/etc/postfix/vmailbox".into(),
            aliases: "/etc/postfix/virtual".into(),
            dovecot_users: "/etc/dovecot/users".into(),
            dovecot_users_gid: Some(dovecot_users_gid),
            mail_root: "/var/mail/vhosts".into(),
            mail_uid: Some(mail_uid),
            mail_gid: Some(mail_gid),
            showq_socket: "/var/spool/postfix/public/showq".into(),
            qmgr_socket: "/var/spool/postfix/public/qmgr".into(),
            postfix_master_pid: Some("/var/spool/postfix/pid/master.pid".into()),
        };
        prepared_mail_owner(&paths)?;
        Ok(paths)
    }
}

fn required_numeric_id(name: &str) -> Result<u32> {
    let value = std::env::var_os(name).with_context(|| format!("{name} is required"))?;
    value
        .to_str()
        .with_context(|| format!("{name} is not UTF-8"))?
        .parse::<u32>()
        .with_context(|| format!("{name} is not numeric"))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct MailDocument {
    domains: BTreeSet<String>,
    mailboxes: BTreeMap<String, StoredMailbox>,
    aliases: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredMailbox {
    domain: String,
    user: String,
    password_hash: String,
    quota_bytes: Option<u64>,
}

#[derive(Clone)]
pub struct MailAdminService {
    documents: Arc<RuntimeDocuments>,
    paths: NativePaths,
    effects: Arc<Mutex<()>>,
}

impl MailAdminService {
    pub fn production() -> Result<Self> {
        let path = std::env::var_os("OP_MAIL_ADMIN_COZO_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/var/lib/op-dbus/mail-admin-cozo".into());
        let store = CozoGraphShuttle::new_persistent_private(path)?;
        Ok(Self::new(
            RuntimeDocuments::new(store),
            NativePaths::production()?,
        ))
    }

    pub fn new(documents: RuntimeDocuments, paths: NativePaths) -> Self {
        Self {
            documents: Arc::new(documents),
            paths,
            effects: Arc::new(Mutex::new(())),
        }
    }

    /// Import the pre-owner flat-file state once, then make Cozo authoritative.
    /// This adoption is explicit at daemon startup and never runs when the
    /// durable document already exists.
    pub async fn adopt_or_reconcile(&self) -> Result<MailAdministrationState> {
        let _guard = self.effects.lock().await;
        let row = if let Some(row) = self.documents.read_durable(DOCUMENT_ID).await? {
            row
        } else {
            let paths = self.paths.clone();
            let imported =
                tokio::task::spawn_blocking(move || import_legacy_document(&paths)).await??;
            self.documents
                .update_at_revision_with_result(DOCUMENT_ID, 0, move |_| {
                    Ok((serde_json::to_value(imported)?, ()))
                })
                .await?
                .0
        };
        let document: MailDocument = serde_json::from_value(row.document)?;
        materialize_async(self.paths.clone(), document.clone()).await?;
        Ok(project_state(
            row.revision,
            &document,
            MailQueueSummary::default(),
        ))
    }

    async fn mutate<F, R>(&self, change: F) -> Result<(u64, MailDocument, R)>
    where
        F: FnOnce(&mut MailDocument) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let _guard = self.effects.lock().await;
        let (row, receipt) = self
            .documents
            .update_with_result(DOCUMENT_ID, move |previous| {
                let mut document = previous
                    .map(serde_json::from_value)
                    .transpose()?
                    .unwrap_or_default();
                let receipt = change(&mut document)?;
                Ok((serde_json::to_value(&document)?, receipt))
            })
            .await?;
        let document: MailDocument = serde_json::from_value(row.document)?;
        materialize_async(self.paths.clone(), document.clone())
            .await
            .context("mail state committed but Postfix/Dovecot materialization failed; retry to reconcile")?;
        Ok((row.revision, document, receipt))
    }

    async fn state_from(
        &self,
        revision: u64,
        document: &MailDocument,
    ) -> Result<MailAdministrationState> {
        let queue = observe_queue(&self.paths.showq_socket).await?.summary;
        Ok(project_state(revision, document, queue))
    }

    async fn state(&self) -> Result<MailAdministrationState> {
        let row = self
            .documents
            .read_durable(DOCUMENT_ID)
            .await?
            .context("mail administration state has not been adopted")?;
        let document = serde_json::from_value(row.document)?;
        self.state_from(row.revision, &document).await
    }

    async fn add_domain_inner(&self, domain: &str) -> Result<DomainMutationOutput> {
        let domain = validate_domain(domain)?;
        let selected = domain.clone();
        let (revision, document, changed) = self
            .mutate(move |document| Ok(document.domains.insert(selected)))
            .await?;
        Ok(DomainMutationOutput {
            domain,
            changed,
            domains: document.domains.into_iter().collect(),
            revision,
        })
    }

    async fn remove_domain_inner(&self, domain: &str) -> Result<DomainMutationOutput> {
        let domain = validate_domain(domain)?;
        let selected = domain.clone();
        let (revision, document, changed) = self
            .mutate(move |document| {
                ensure!(
                    !document
                        .mailboxes
                        .values()
                        .any(|mailbox| mailbox.domain == selected),
                    "domain still owns mailboxes"
                );
                ensure!(
                    !document
                        .aliases
                        .keys()
                        .any(|address| address.ends_with(&format!("@{selected}"))),
                    "domain still owns aliases"
                );
                Ok(document.domains.remove(&selected))
            })
            .await?;
        Ok(DomainMutationOutput {
            domain,
            changed,
            domains: document.domains.into_iter().collect(),
            revision,
        })
    }

    async fn add_mailbox_inner(
        &self,
        domain: &str,
        user: &str,
        password_hash: &str,
    ) -> Result<MailboxMutationOutput> {
        let (domain, user, address) = validate_mailbox_key(domain, user)?;
        validate_password_hash(password_hash)?;
        let selected_address = address.clone();
        let selected_domain = domain.clone();
        let selected_user = user.clone();
        let password_hash = password_hash.to_owned();
        let (revision, document, changed) = self
            .mutate(move |document| {
                ensure!(
                    document.domains.contains(&selected_domain),
                    "mail domain does not exist"
                );
                ensure!(
                    !document.aliases.contains_key(&selected_address),
                    "address is already an alias"
                );
                if let Some(existing) = document.mailboxes.get(&selected_address) {
                    ensure!(
                        existing.password_hash == password_hash,
                        "mailbox already exists with a different credential"
                    );
                    return Ok(false);
                }
                document.mailboxes.insert(
                    selected_address,
                    StoredMailbox {
                        domain: selected_domain,
                        user: selected_user,
                        password_hash,
                        quota_bytes: None,
                    },
                );
                Ok(true)
            })
            .await?;
        let mailbox = document
            .mailboxes
            .get(&address)
            .context("mailbox readback missing")?;
        Ok(MailboxMutationOutput {
            address,
            maildir: maildir_for(mailbox),
            quota_bytes: mailbox.quota_bytes,
            changed,
            maildir_preserved: false,
            revision,
        })
    }

    async fn remove_mailbox_inner(
        &self,
        domain: &str,
        user: &str,
    ) -> Result<MailboxMutationOutput> {
        let (domain, user, address) = validate_mailbox_key(domain, user)?;
        let selected = address.clone();
        let (revision, _document, removed) = self
            .mutate(move |document| {
                ensure!(
                    !document
                        .aliases
                        .values()
                        .any(|destination| destination == &selected),
                    "mailbox is still an alias destination"
                );
                Ok(document.mailboxes.remove(&selected))
            })
            .await?;
        Ok(MailboxMutationOutput {
            address,
            maildir: format!("{domain}/{user}/Maildir/"),
            quota_bytes: removed.as_ref().and_then(|mailbox| mailbox.quota_bytes),
            changed: removed.is_some(),
            maildir_preserved: true,
            revision,
        })
    }

    async fn add_alias_inner(
        &self,
        domain: &str,
        alias: &str,
        destination: &str,
    ) -> Result<AliasMutationOutput> {
        let (_domain, _alias, address) = validate_mailbox_key(domain, alias)?;
        let destination = validate_address(destination)?;
        let selected_address = address.clone();
        let selected_destination = destination.clone();
        let domain = validate_domain(domain)?;
        let (revision, _document, changed) = self
            .mutate(move |document| {
                ensure!(
                    document.domains.contains(&domain),
                    "mail domain does not exist"
                );
                ensure!(
                    !document.mailboxes.contains_key(&selected_address),
                    "address is already a mailbox"
                );
                let changed =
                    document.aliases.get(&selected_address) != Some(&selected_destination);
                document
                    .aliases
                    .insert(selected_address, selected_destination);
                Ok(changed)
            })
            .await?;
        Ok(AliasMutationOutput {
            address,
            destination: Some(destination),
            changed,
            revision,
        })
    }

    async fn remove_alias_inner(&self, domain: &str, alias: &str) -> Result<AliasMutationOutput> {
        let (_domain, _alias, address) = validate_mailbox_key(domain, alias)?;
        let selected = address.clone();
        let (revision, _document, removed) = self
            .mutate(move |document| Ok(document.aliases.remove(&selected)))
            .await?;
        Ok(AliasMutationOutput {
            address,
            destination: removed.clone(),
            changed: removed.is_some(),
            revision,
        })
    }

    async fn set_quota_inner(
        &self,
        domain: &str,
        user: &str,
        quota_mb: u32,
    ) -> Result<QuotaMutationOutput> {
        ensure!(
            quota_mb > 0 && quota_mb <= 1_048_576,
            "quota_mb must be between 1 and 1048576"
        );
        let (_domain, _user, address) = validate_mailbox_key(domain, user)?;
        let quota_bytes = u64::from(quota_mb) * 1024 * 1024;
        let selected = address.clone();
        let (revision, _document, ()) = self
            .mutate(move |document| {
                let mailbox = document
                    .mailboxes
                    .get_mut(&selected)
                    .context("mailbox does not exist")?;
                mailbox.quota_bytes = Some(quota_bytes);
                Ok(())
            })
            .await?;
        Ok(QuotaMutationOutput {
            address,
            quota_bytes,
            revision,
        })
    }

    async fn get_queue_inner(&self) -> Result<GetQueueOutput> {
        observe_queue(&self.paths.showq_socket).await
    }

    async fn flush_queue_inner(&self) -> Result<FlushQueueOutput> {
        let _guard = self.effects.lock().await;
        let before = observe_queue(&self.paths.showq_socket).await?.summary;
        trigger_queue_flush(&self.paths.qmgr_socket).await?;
        let after = observe_queue(&self.paths.showq_socket).await?.summary;
        Ok(FlushQueueOutput {
            accepted: true,
            before,
            after,
        })
    }
}

#[zbus::interface(name = "org.opdbus.v1.plugins.mail_server.Native")]
impl MailAdminService {
    async fn add_domain(&self, domain: &str) -> zbus::fdo::Result<String> {
        json_reply(self.add_domain_inner(domain).await)
    }
    async fn remove_domain(&self, domain: &str) -> zbus::fdo::Result<String> {
        json_reply(self.remove_domain_inner(domain).await)
    }
    async fn add_mailbox(
        &self,
        domain: &str,
        user: &str,
        password_hash: &str,
    ) -> zbus::fdo::Result<String> {
        json_reply(self.add_mailbox_inner(domain, user, password_hash).await)
    }
    async fn remove_mailbox(&self, domain: &str, user: &str) -> zbus::fdo::Result<String> {
        json_reply(self.remove_mailbox_inner(domain, user).await)
    }
    async fn add_alias(
        &self,
        domain: &str,
        alias: &str,
        destination: &str,
    ) -> zbus::fdo::Result<String> {
        json_reply(self.add_alias_inner(domain, alias, destination).await)
    }
    async fn remove_alias(&self, domain: &str, alias: &str) -> zbus::fdo::Result<String> {
        json_reply(self.remove_alias_inner(domain, alias).await)
    }
    async fn set_quota(
        &self,
        domain: &str,
        user: &str,
        quota_mb: u32,
    ) -> zbus::fdo::Result<String> {
        json_reply(self.set_quota_inner(domain, user, quota_mb).await)
    }
    async fn get_queue(&self) -> zbus::fdo::Result<String> {
        json_reply(self.get_queue_inner().await)
    }
    async fn flush_queue(&self) -> zbus::fdo::Result<String> {
        json_reply(self.flush_queue_inner().await)
    }
    async fn get_state(&self) -> zbus::fdo::Result<String> {
        json_reply(self.state().await)
    }
}

fn json_reply<T: Serialize>(value: Result<T>) -> zbus::fdo::Result<String> {
    value
        .and_then(|value| serde_json::to_string(&value).map_err(Into::into))
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
}

fn validate_domain(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_lowercase();
    ensure!(
        !value.is_empty() && value.len() <= 253,
        "invalid mail domain length"
    );
    ensure!(
        !value.ends_with('.') && value.contains('.'),
        "mail domain must be a fully-qualified DNS name"
    );
    ensure!(
        value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        }),
        "mail domain contains an invalid DNS label"
    );
    Ok(value)
}

fn validate_localpart(value: &str) -> Result<String> {
    let value = value.trim().to_ascii_lowercase();
    ensure!(
        !value.is_empty() && value.len() <= 64,
        "invalid mailbox local-part length"
    );
    ensure!(
        value.bytes().all(|byte| byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || b".!#$%&'*+-/=?^_`{|}~".contains(&byte))
            && !value.starts_with('.')
            && !value.ends_with('.')
            && !value.contains(".."),
        "mailbox local-part contains unsupported characters"
    );
    Ok(value)
}

fn validate_mailbox_key(domain: &str, user: &str) -> Result<(String, String, String)> {
    let domain = validate_domain(domain)?;
    let user = validate_localpart(user)?;
    let address = format!("{user}@{domain}");
    Ok((domain, user, address))
}

fn validate_address(value: &str) -> Result<String> {
    let (local, domain) = value
        .trim()
        .rsplit_once('@')
        .context("destination must be one email address")?;
    Ok(format!(
        "{}@{}",
        validate_localpart(local)?,
        validate_domain(domain)?
    ))
}

fn validate_password_hash(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 4096
            && !value.contains(':')
            && !value.chars().any(char::is_control),
        "invalid Dovecot password hash"
    );
    ensure!(
        value.starts_with('$') || (value.starts_with('{') && value.contains('}')),
        "password_hash must identify a crypt or Dovecot password scheme"
    );
    ensure!(
        !value.to_ascii_uppercase().starts_with("{PLAIN}"),
        "plaintext password schemes are not accepted"
    );
    Ok(())
}

fn maildir_for(mailbox: &StoredMailbox) -> String {
    format!("{}/{}/Maildir/", mailbox.domain, mailbox.user)
}

fn project_state(
    revision: u64,
    document: &MailDocument,
    queue: MailQueueSummary,
) -> MailAdministrationState {
    MailAdministrationState {
        revision,
        domains: document.domains.iter().cloned().collect(),
        mailboxes: document
            .mailboxes
            .iter()
            .map(|(address, mailbox)| MailMailbox {
                address: address.clone(),
                maildir: maildir_for(mailbox),
                quota_bytes: mailbox.quota_bytes,
            })
            .collect(),
        aliases: document
            .aliases
            .iter()
            .map(|(address, destination)| MailAlias {
                address: address.clone(),
                destination: destination.clone(),
            })
            .collect(),
        queue,
    }
}

fn import_legacy_document(paths: &NativePaths) -> Result<MailDocument> {
    let mut document = MailDocument::default();
    let domain_lines = bounded_read(&paths.domains).unwrap_or_default();
    let mailbox_lines = bounded_read(&paths.mailboxes)?;
    let password_lines = bounded_read(&paths.dovecot_users)?;
    let alias_lines = bounded_read(&paths.aliases).unwrap_or_default();
    let passwords = parse_dovecot_users(&password_lines)?;
    for (domain, _) in parse_table(&domain_lines)? {
        document.domains.insert(validate_domain(&domain)?);
    }
    for (address, maildir) in parse_table(&mailbox_lines)? {
        let address = validate_address(&address)?;
        let (user, domain) = address.rsplit_once('@').context("mailbox address")?;
        let user = user.to_owned();
        let domain = domain.to_owned();
        let password = passwords
            .get(&address)
            .context("mailbox has no Dovecot credential")?;
        document.domains.insert(domain.clone());
        document.mailboxes.insert(
            address,
            StoredMailbox {
                domain,
                user,
                password_hash: password.0.clone(),
                quota_bytes: password.1,
            },
        );
        ensure!(
            maildir.ends_with("Maildir/"),
            "legacy mailbox is not Maildir-backed"
        );
    }
    for (address, destination) in parse_table(&alias_lines)? {
        let address = validate_address(&address)?;
        let destination = validate_address(&destination)?;
        if let Some((_, domain)) = address.rsplit_once('@') {
            document.domains.insert(domain.to_owned());
        }
        ensure!(
            !document.mailboxes.contains_key(&address),
            "legacy alias collides with mailbox"
        );
        document.aliases.insert(address, destination);
    }
    Ok(document)
}

fn bounded_read(path: &Path) -> Result<String> {
    let metadata =
        std::fs::metadata(path).with_context(|| format!("inspect {}", path.display()))?;
    ensure!(
        metadata.len() <= MAX_CONFIG_BYTES as u64,
        "{} exceeds the config size limit",
        path.display()
    );
    std::fs::read_to_string(path).with_context(|| format!("read legacy {}", path.display()))
}

fn parse_table(text: &str) -> Result<Vec<(String, String)>> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            (!line.is_empty() && !line.starts_with('#')).then_some(line)
        })
        .map(|line| {
            let (key, value) = line
                .split_once(char::is_whitespace)
                .context("mail table line has no value")?;
            Ok((key.to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn parse_dovecot_users(text: &str) -> Result<BTreeMap<String, (String, Option<u64>)>> {
    let mut users = BTreeMap::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let mut fields = line.splitn(8, ':');
        let address = validate_address(fields.next().unwrap_or_default())?;
        let password = fields
            .next()
            .context("Dovecot passwd entry has no password")?
            .to_owned();
        validate_password_hash(&password)?;
        let tail = fields.nth(5).unwrap_or_default();
        let quota_bytes = tail
            .split_whitespace()
            .find_map(|field| field.strip_prefix("userdb_quota_rule=*:bytes="))
            .map(str::parse::<u64>)
            .transpose()
            .context("invalid Dovecot quota bytes")?;
        ensure!(
            users.insert(address, (password, quota_bytes)).is_none(),
            "duplicate Dovecot user"
        );
    }
    Ok(users)
}

async fn materialize_async(paths: NativePaths, document: MailDocument) -> Result<()> {
    tokio::task::spawn_blocking(move || materialize(&paths, &document)).await?
}

fn materialize(paths: &NativePaths, document: &MailDocument) -> Result<()> {
    let (uid, gid) = prepared_mail_owner(paths)?;
    let domains = document
        .domains
        .iter()
        .map(|domain| format!("{domain}\tOK\n"))
        .collect::<String>();
    let mailboxes = document
        .mailboxes
        .iter()
        .map(|(address, mailbox)| format!("{address}\t{}\n", maildir_for(mailbox)))
        .collect::<String>();
    let aliases = document
        .aliases
        .iter()
        .map(|(address, destination)| format!("{address}\t{destination}\n"))
        .collect::<String>();
    let users = document
        .mailboxes
        .iter()
        .map(|(address, mailbox)| {
            let quota = mailbox
                .quota_bytes
                .map(|bytes| format!("userdb_quota_rule=*:bytes={bytes}"))
                .unwrap_or_default();
            format!("{address}:{}::::::{quota}\n", mailbox.password_hash)
        })
        .collect::<String>();
    atomic_write(&paths.domains, domains.as_bytes(), 0o644)?;
    atomic_write(&paths.mailboxes, mailboxes.as_bytes(), 0o644)?;
    atomic_write(&paths.aliases, aliases.as_bytes(), 0o644)?;
    atomic_write(&paths.dovecot_users, users.as_bytes(), 0o640)?;
    if let Some(gid) = paths.dovecot_users_gid {
        nix::unistd::chown(
            &paths.dovecot_users,
            None,
            Some(nix::unistd::Gid::from_raw(gid)),
        )
        .with_context(|| {
            format!(
                "set Dovecot reader group on {}",
                paths.dovecot_users.display()
            )
        })?;
    }
    for mailbox in document.mailboxes.values() {
        let home = paths.mail_root.join(&mailbox.domain).join(&mailbox.user);
        let maildir = home.join("Maildir");
        for path in [
            paths.mail_root.join(&mailbox.domain),
            home,
            maildir.clone(),
            maildir.join("cur"),
            maildir.join("new"),
            maildir.join("tmp"),
        ] {
            std::fs::create_dir_all(&path).with_context(|| format!("create {}", path.display()))?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
            nix::unistd::chown(&path, Some(uid), Some(gid))
                .with_context(|| format!("set vmail ownership on {}", path.display()))?;
        }
    }
    verify_materialized(paths, document)?;
    if let Some(pid_path) = &paths.postfix_master_pid {
        if pid_path.exists() {
            reload_postfix(pid_path)?;
        }
    }
    Ok(())
}

fn prepared_mail_owner(paths: &NativePaths) -> Result<(nix::unistd::Uid, nix::unistd::Gid)> {
    match (paths.mail_uid, paths.mail_gid) {
        (Some(uid), Some(gid)) => {
            ensure!(uid != 0, "virtual mail owner must not be root");
            ensure!(gid != 0, "virtual mail group must not be root");
            let metadata = std::fs::metadata(&paths.mail_root).with_context(|| {
                format!(
                    "prepared virtual mail root is absent: {}",
                    paths.mail_root.display()
                )
            })?;
            ensure!(
                metadata.is_dir(),
                "prepared virtual mail root is not a directory"
            );
            ensure!(
                metadata.uid() == uid && metadata.gid() == gid,
                "prepared virtual mail root owner is {}:{}, expected {uid}:{gid}",
                metadata.uid(),
                metadata.gid()
            );
            ensure!(
                metadata.permissions().mode() & 0o777 == 0o700,
                "prepared virtual mail root mode is {:04o}, expected 0700",
                metadata.permissions().mode() & 0o777
            );
            Ok((
                nix::unistd::Uid::from_raw(uid),
                nix::unistd::Gid::from_raw(gid),
            ))
        }
        (None, None) => {
            std::fs::create_dir_all(&paths.mail_root)?;
            let metadata = std::fs::metadata(&paths.mail_root)?;
            Ok((
                nix::unistd::Uid::from_raw(metadata.uid()),
                nix::unistd::Gid::from_raw(metadata.gid()),
            ))
        }
        _ => anyhow::bail!("virtual mail owner requires both numeric uid and gid"),
    }
}

fn verify_materialized(paths: &NativePaths, expected: &MailDocument) -> Result<()> {
    let domains = parse_table(&bounded_read(&paths.domains)?)?
        .into_iter()
        .map(|(domain, value)| {
            ensure!(
                value == "OK",
                "Postfix domain table contains a non-OK value"
            );
            validate_domain(&domain)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        domains == expected.domains,
        "Postfix domain table readback differs"
    );

    let mailboxes = parse_table(&bounded_read(&paths.mailboxes)?)?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let expected_mailboxes = expected
        .mailboxes
        .iter()
        .map(|(address, mailbox)| (address.clone(), maildir_for(mailbox)))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        mailboxes == expected_mailboxes,
        "Postfix mailbox table readback differs"
    );

    let aliases = parse_table(&bounded_read(&paths.aliases)?)?
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    ensure!(
        aliases == expected.aliases,
        "Postfix alias table readback differs"
    );

    let users = parse_dovecot_users(&bounded_read(&paths.dovecot_users)?)?;
    let expected_users = expected
        .mailboxes
        .iter()
        .map(|(address, mailbox)| {
            (
                address.clone(),
                (mailbox.password_hash.clone(), mailbox.quota_bytes),
            )
        })
        .collect::<BTreeMap<_, _>>();
    ensure!(
        users == expected_users,
        "Dovecot passwd/quota readback differs"
    );
    Ok(())
}

fn reload_postfix(pid_path: &Path) -> Result<()> {
    let pid_text = bounded_read(pid_path)?;
    let pid = pid_text
        .trim()
        .parse::<i32>()
        .context("Postfix master PID is invalid")?;
    ensure!(pid > 1, "Postfix master PID is unsafe");
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGHUP,
    )
    .context("reload Postfix after mail table replacement")
}

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    ensure!(
        bytes.len() <= MAX_CONFIG_BYTES,
        "rendered mail config exceeds size limit"
    );
    let parent = path
        .parent()
        .context("mail config has no parent directory")?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("mail"),
        uuid::Uuid::new_v4()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(mode);
    let mut file = options
        .open(&temp)
        .with_context(|| format!("create {}", temp.display()))?;
    use std::io::Write;
    if let Err(error) = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        std::fs::File::open(parent)?.sync_all()
    })() {
        let _ = std::fs::remove_file(&temp);
        return Err(error).with_context(|| format!("atomically replace {}", path.display()));
    }
    Ok(())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

async fn observe_queue(path: &Path) -> Result<GetQueueOutput> {
    let mut stream = tokio::time::timeout(IO_TIMEOUT, UnixStream::connect(path))
        .await
        .context("Postfix showq connect timed out")?
        .with_context(|| format!("connect to Postfix showq at {}", path.display()))?;
    let mut bytes = Vec::new();
    tokio::time::timeout(IO_TIMEOUT, stream.read_to_end(&mut bytes))
        .await
        .context("Postfix showq read timed out")??;
    ensure!(
        bytes.len() <= MAX_QUEUE_BYTES,
        "Postfix queue response exceeds size limit"
    );
    parse_showq(&bytes)
}

fn parse_showq(bytes: &[u8]) -> Result<GetQueueOutput> {
    let tokens = bytes
        .split(|byte| *byte == 0)
        .map(|token| {
            String::from_utf8(token.to_vec()).context("Postfix showq returned non-UTF-8 data")
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        tokens.first().map(String::as_str) == Some("protocol")
            && tokens.get(1).map(String::as_str) == Some("mail_queue_list_protocol"),
        "unexpected Postfix showq protocol"
    );
    let mut index = 2;
    expect_empty(&tokens, &mut index, "protocol terminator")?;
    let mut entries = Vec::new();
    while index < tokens.len() && !tokens[index].is_empty() {
        let queue_name = take_attr(&tokens, &mut index, "queue_name")?;
        let queue_id = take_attr(&tokens, &mut index, "queue_id")?;
        let arrival_time = take_attr(&tokens, &mut index, "time")?
            .parse::<i64>()
            .context("invalid Postfix showq time")?;
        let message_size = take_attr(&tokens, &mut index, "size")?
            .parse::<u64>()
            .context("invalid Postfix showq size")?;
        let forced_expire = match take_attr(&tokens, &mut index, "forced_expire")?.as_str() {
            "0" => false,
            "1" => true,
            _ => anyhow::bail!("invalid Postfix showq forced_expire"),
        };
        let sender = take_attr(&tokens, &mut index, "sender")?;
        let mut recipients = Vec::new();
        while index < tokens.len() && !tokens[index].is_empty() {
            let original = take_attr(&tokens, &mut index, "original_recipient")?;
            let address = take_attr(&tokens, &mut index, "recipient")?;
            let reason_kind = take_attr(&tokens, &mut index, "log_class")?;
            let reason = take_attr(&tokens, &mut index, "reason")?;
            recipients.push(MailQueueRecipient {
                address,
                original_address: (!original.is_empty()).then_some(original),
                reason_kind: (!reason_kind.is_empty()).then_some(reason_kind),
                delay_reason: (!reason.is_empty()).then_some(reason),
            });
        }
        expect_empty(&tokens, &mut index, "message terminator")?;
        entries.push(MailQueueEntry {
            queue_name,
            queue_id,
            arrival_time,
            message_size,
            forced_expire,
            sender,
            recipients,
        });
    }
    expect_empty(&tokens, &mut index, "queue terminator")?;
    ensure!(
        tokens[index..].iter().all(String::is_empty),
        "unexpected trailing Postfix showq data"
    );
    let summary = MailQueueSummary {
        messages: entries.len() as u64,
        bytes: entries.iter().map(|entry| entry.message_size).sum(),
        observed_at_unix: now_unix(),
    };
    Ok(GetQueueOutput { entries, summary })
}

fn take_attr(tokens: &[String], index: &mut usize, expected: &str) -> Result<String> {
    ensure!(
        tokens.get(*index).map(String::as_str) == Some(expected),
        "Postfix showq expected {expected}"
    );
    let value = tokens
        .get(*index + 1)
        .context("truncated Postfix showq attribute")?
        .clone();
    *index += 2;
    Ok(value)
}

fn expect_empty(tokens: &[String], index: &mut usize, label: &str) -> Result<()> {
    ensure!(
        tokens.get(*index).is_some_and(String::is_empty),
        "Postfix showq omitted {label}"
    );
    *index += 1;
    Ok(())
}

async fn trigger_queue_flush(path: &Path) -> Result<()> {
    let mut stream = tokio::time::timeout(IO_TIMEOUT, UnixStream::connect(path))
        .await
        .context("Postfix qmgr connect timed out")?
        .with_context(|| format!("connect to Postfix qmgr at {}", path.display()))?;
    // mail_flush_deferred(): FLUSH_DEAD + SCAN_ALL + SCAN_DEFERRED +
    // SCAN_INCOMING, followed by unix_trigger's NUL terminator.
    tokio::time::timeout(IO_TIMEOUT, stream.write_all(b"FADI\0"))
        .await
        .context("Postfix qmgr flush write timed out")??;
    stream.shutdown().await?;
    Ok(())
}

#[cfg(test)]
#[path = "native_owner_tests.rs"]
mod tests;
