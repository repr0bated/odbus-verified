//! opblob — drive the schema→blob→catalog→btrfs pipeline from the shell.
//!
//! ```text
//! opblob demo-seal <dir>              blobify demo plugins into a catalog dir
//! opblob seal-shm                     blobify real plugins into /dev/shm/opdbus/plugin-blobs
//! opblob stage-shm                    stage the persisted catalog into SHM (boot path)
//! opblob persist                      copy the SHM catalog to the persisted store
//! opblob persist <source> <dest>      atomically copy an isolated sealed catalog
//! opblob seal-plugins <dir>           blobify real plugins into a catalog dir
//! opblob inspect <file.blob>          print identity, services, methods
//! opblob catalog <dir>                list active services in a catalog dir
//! opblob btrfs-seal <image> <dir>     seal a catalog dir into a btrfs image
//! opblob keygen <keyfile>             generate the account WG identity keypair
//! ```
//!
//! `persist` and `stage-shm` accept `OPDBUS_RELEASE_CATALOG_STAGE` for a
//! serialized release controller: an already journaled, empty, root-owned
//! canonical sibling directory. This mode never removes directories or rolls
//! back a rename, even on error. The controller must reconcile directory
//! device/inode identities after the publisher exits, not trust its exit code.

use op_blob::{
    BlobRef, blob, btrfs, try_blobify_plugin_schema,
    catalog::{ActiveReflectionCatalog, DEFAULT_SHM_DIR},
    demo, descriptor,
    identity::WgKeypair,
};
use op_plugins::DefaultPluginRegistry;
use op_state_store::MemoryStore;
use std::process::ExitCode;
use std::sync::Arc;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [cmd, dir] if cmd == "demo-seal" => demo_seal(Path::new(dir)),
        [cmd] if cmd == "seal-shm" => seal_plugins(Path::new(DEFAULT_SHM_DIR)),
        [cmd] if cmd == "stage-shm" => stage_shm(),
        [cmd] if cmd == "persist" => {
            persist_catalog(Path::new(DEFAULT_SHM_DIR), Path::new(PERSISTED_CATALOG_DIR))
        }
        [cmd, source, destination] if cmd == "persist" => {
            persist_catalog(Path::new(source), Path::new(destination))
        }
        [cmd, dir] if cmd == "seal-plugins" => seal_plugins(Path::new(dir)),
        [cmd, file] if cmd == "inspect" => inspect(Path::new(file)),
        [cmd, dir] if cmd == "catalog" => catalog(Path::new(dir)),
        [cmd, image, dir] if cmd == "btrfs-seal" => btrfs_seal(Path::new(image), Path::new(dir)),
        [cmd, keyfile] if cmd == "keygen" => keygen(Path::new(keyfile)),
        _ => {
            eprintln!(
                "usage: opblob demo-seal <dir> | seal-shm | stage-shm | persist [<source> <destination>] | seal-plugins <dir> | inspect <file.blob> | catalog <dir> | btrfs-seal <image> <dir> | keygen <keyfile>"
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("opblob: {e}");
            ExitCode::FAILURE
        }
    }
}

fn account_identity(dir: &Path) -> Result<WgKeypair, String> {
    // The account keypair is the identity; the session it defines persists
    // for the life of the account, so reuse the key if present.
    let keyfile = dir.join("identity.key");
    if keyfile.exists() {
        WgKeypair::load(&keyfile).map_err(|e| e.to_string())
    } else {
        let kp = WgKeypair::generate();
        kp.save(&keyfile).map_err(|e| e.to_string())?;
        Ok(kp)
    }
}

fn demo_seal(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let kp = account_identity(dir)?;
    let id = kp.public_identity();
    println!(
        "identity  wg-pub {}  key-id {}",
        id.wg_public_key, id.key_id
    );
    println!("session   {} (account-persistent)", id.session.session_id);

    let candidates = [demo::unix_socket_schema(), demo::wireguard_schema()]
        .iter()
        .map(|schema| blob::try_blobify_with_identity(schema, Some(id.clone())))
        .collect::<Result<Vec<_>, _>>()?;
    let mut cat = ActiveReflectionCatalog::open(dir).map_err(|e| e.to_string())?;
    for b in candidates {
        let path = cat.upsert_blob(&b).map_err(|e| e.to_string())?;
        println!(
            "sealed    {}  ({} methods, schema {})",
            path.display(),
            b.manifest.methods.len(),
            &b.manifest.schema_hash[..16]
        );
    }
    println!("services:");
    for s in cat.list_services() {
        println!("  {s}");
    }
    Ok(())
}

/// Where the sealed catalog survives a reboot. `/dev/shm` is tmpfs, so without
/// a persisted copy the only way to refill it is to re-seal — which means
/// loading every plugin and probing live backends on the boot path.
const PERSISTED_CATALOG_DIR: &str = "/var/lib/opdbus/plugin-blobs";

/// Catalog manifest filename — written LAST, as the atomic commit point that
/// readers watch for. Mirrors `op_blob::catalog`'s own contract.
const MANIFEST_NAME: &str = ".manifest.json";

/// Boot path: stage the persisted catalog into SHM without regenerating it.
///
/// Sealing is a build-time act — it loads every plugin and calls
/// `schema_live()`, which probes real backends. Doing that at boot makes
/// startup depend on service ordering and costs minutes. Staging is a copy.
///
/// Requires a previously sealed and persisted catalog, including on first boot.
/// Missing release inputs must not silently create a different plugin set.
fn stage_shm() -> Result<(), String> {
    let shm = Path::new(DEFAULT_SHM_DIR);
    let persisted = Path::new(PERSISTED_CATALOG_DIR);

    if !persisted.join(MANIFEST_NAME).is_file() {
        return Err(format!(
            "no persisted catalog at {}; explicitly seal and persist the prepared release before staging",
            persisted.display()
        ));
    }

    copy_catalog(persisted, shm)?;
    println!("staged    {} -> {}", persisted.display(), shm.display());
    Ok(())
}

/// Capture the current SHM catalog as the artifact future boots will stage.
/// Run this after a deliberate `seal-shm`, not on the boot path.
fn persist_catalog(shm: &Path, persisted: &Path) -> Result<(), String> {
    if !shm.join(MANIFEST_NAME).is_file() {
        return Err(format!(
            "no catalog to persist: {} has no {MANIFEST_NAME}",
            shm.display()
        ));
    }
    copy_catalog(shm, persisted)?;
    println!("persisted {} -> {}", shm.display(), persisted.display());
    Ok(())
}

#[derive(Debug)]
struct CatalogFile {
    name: String,
    bytes: Vec<u8>,
    mode: u32,
}

#[derive(Debug)]
struct ValidatedCatalog {
    manifest: Vec<u8>,
    manifest_mode: u32,
    directory_mode: u32,
    blobs: Vec<CatalogFile>,
}

/// Validate and capture one complete committed catalog before touching its
/// destination. The captured bytes close the source validation/copy race.
fn validate_catalog(src: &Path) -> Result<ValidatedCatalog, String> {
    let src = std::fs::canonicalize(src)
        .map_err(|error| format!("resolve source catalog {}: {error}", src.display()))?;
    let source_metadata = std::fs::symlink_metadata(&src)
        .map_err(|error| format!("inspect source catalog {}: {error}", src.display()))?;
    if !source_metadata.is_dir() {
        return Err(format!(
            "source catalog is not a directory: {}",
            src.display()
        ));
    }

    let manifest_path = src.join(MANIFEST_NAME);
    let manifest_metadata = std::fs::symlink_metadata(&manifest_path)
        .map_err(|error| format!("inspect {}: {error}", manifest_path.display()))?;
    if !manifest_metadata.is_file() {
        return Err(format!(
            "catalog manifest is not a regular file: {}",
            manifest_path.display()
        ));
    }
    let manifest = std::fs::read(&manifest_path)
        .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
    let document: serde_json::Value = serde_json::from_slice(&manifest)
        .map_err(|error| format!("parse {}: {error}", manifest_path.display()))?;
    let catalog_hash = document
        .get("catalog_hash")
        .and_then(serde_json::Value::as_str)
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| "catalog manifest has no valid catalog_hash".to_string())?;
    let plugin_values = document
        .get("plugins")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "catalog manifest has no plugins object".to_string())?;
    let mut plugins = BTreeMap::new();
    for (plugin_id, value) in plugin_values {
        if plugin_id.is_empty()
            || !plugin_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            || blob::canonical_plugin_id(plugin_id) != *plugin_id
        {
            return Err(format!(
                "catalog manifest has invalid plugin id {plugin_id:?}"
            ));
        }
        let schema_hash = value
            .as_str()
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| format!("catalog manifest has invalid hash for {plugin_id}"))?;
        plugins.insert(plugin_id.clone(), schema_hash.to_ascii_lowercase());
    }

    let expected_names: BTreeSet<String> = plugins
        .iter()
        .map(|(plugin_id, schema_hash)| format!("{plugin_id}.{}.blob", &schema_hash[..16]))
        .collect();
    let mut actual_names = BTreeSet::new();
    for entry in std::fs::read_dir(&src)
        .map_err(|error| format!("read source catalog {}: {error}", src.display()))?
    {
        let entry = entry.map_err(|error| format!("read source catalog entry: {error}"))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("blob") {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("inspect {}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!(
                "catalog blob is not a regular file: {}",
                path.display()
            ));
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "catalog blob filename is not UTF-8".to_string())?;
        actual_names.insert(name);
    }
    if actual_names != expected_names {
        return Err("catalog blob files differ from the committed manifest".into());
    }

    let mut blobs = Vec::with_capacity(plugins.len());
    for (plugin_id, schema_hash) in &plugins {
        let name = format!("{plugin_id}.{}.blob", &schema_hash[..16]);
        let path = src.join(&name);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("inspect {}: {error}", path.display()))?;
        let bytes =
            std::fs::read(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
        let sealed = BlobRef::new(&bytes)
            .map_err(|error| format!("validate {}: {error}", path.display()))?;
        if sealed.schema_hash_hex() != *schema_hash {
            return Err(format!("{name} differs from the manifest schema hash"));
        }
        let embedded = sealed
            .manifest()
            .map_err(|error| format!("parse {name} manifest: {error}"))?;
        if embedded.plugin_id != *plugin_id
            || embedded.schema_hash != *schema_hash
            || embedded.blob_type != "active_reflection"
        {
            return Err(format!("{name} has inconsistent embedded identity"));
        }
        let schema = sealed
            .state_store_schema()
            .map_err(|error| format!("parse {name} schema: {error}"))?;
        if schema.name != *plugin_id || schema.version != embedded.schema_version {
            return Err(format!("{name} schema identity differs from its manifest"));
        }
        blobs.push(CatalogFile {
            name,
            bytes,
            mode: metadata.permissions().mode(),
        });
    }

    let indexed = ActiveReflectionCatalog::open(&src)
        .map_err(|error| format!("index source catalog {}: {error}", src.display()))?;
    if indexed.active_plugins() != plugins.keys().cloned().collect::<Vec<_>>()
        || indexed.catalog_hash() != catalog_hash.to_ascii_lowercase()
    {
        return Err("catalog manifest identity differs from its sealed blobs".into());
    }
    if std::fs::read(&manifest_path)
        .map_err(|error| format!("re-read {}: {error}", manifest_path.display()))?
        != manifest
    {
        return Err("catalog manifest changed during validation".into());
    }

    Ok(ValidatedCatalog {
        manifest,
        manifest_mode: manifest_metadata.permissions().mode(),
        directory_mode: source_metadata.permissions().mode(),
        blobs,
    })
}

fn write_synced(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("write {}: {error}", path.display()))?;
    file.set_permissions(std::fs::Permissions::from_mode(mode))
        .map_err(|error| format!("chmod {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("sync {}: {error}", path.display()))
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("sync directory {}: {error}", path.display()))
}

fn directory_identity(metadata: &std::fs::Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

fn catalog_directory_identity(path: &Path) -> Result<Option<(u64, u64)>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(Some(directory_identity(&metadata))),
        Ok(_) => Err(format!("not a nonsymlink catalog directory: {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("inspect directory {}: {error}", path.display())),
    }
}

fn require_directory_identity(path: &Path, expected: Option<(u64, u64)>) -> Result<(), String> {
    if catalog_directory_identity(path)? != expected {
        return Err(format!("catalog directory identity changed: {}", path.display()));
    }
    Ok(())
}

fn open_release_directory(path: &Path) -> Result<File, String> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| format!("open release directory {}: {error}", path.display()))?;
    let metadata = directory
        .metadata()
        .map_err(|error| format!("inspect release directory {}: {error}", path.display()))?;
    if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err(format!(
            "release directory must be root-owned and not group/other writable: {}",
            path.display()
        ));
    }
    require_directory_identity(path, Some(directory_identity(&metadata)))?;
    Ok(directory)
}

/// Named stage entries are always complete source bytes. A killed/failed write
/// leaves only an anonymous inode, released when its last fd closes; recovery
/// never needs to authorize deletion of a partially written named file.
fn write_release_file(
    directory: &File,
    directory_path: &Path,
    name: &str,
    bytes: &[u8],
    mode: u32,
) -> Result<(), String> {
    let name = std::ffi::CString::new(name)
        .map_err(|_| "catalog filename contains NUL".to_string())?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_TMPFILE)
        .open(directory_path)
        .map_err(|error| format!("create anonymous catalog file: {error}"))?;
    if file.metadata().map_err(|error| error.to_string())?.uid() != 0 {
        return Err("coordinated catalog files must be created as root".into());
    }
    file.write_all(bytes)
        .map_err(|error| format!("write anonymous catalog file: {error}"))?;
    file.set_permissions(std::fs::Permissions::from_mode(mode))
        .map_err(|error| format!("chmod anonymous catalog file: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync anonymous catalog file: {error}"))?;
    // SAFETY: both descriptors remain live, the empty source and destination
    // name are NUL-terminated, and linkat does not retain these pointers.
    let result = unsafe {
        nix::libc::linkat(
            file.as_raw_fd(),
            c"".as_ptr(),
            directory.as_raw_fd(),
            name.as_ptr(),
            nix::libc::AT_EMPTY_PATH,
        )
    };
    if result != 0 {
        return Err(format!(
            "link completed catalog file {:?}: {}",
            name,
            std::io::Error::last_os_error()
        ));
    }
    file.sync_all()
        .map_err(|error| format!("sync linked catalog file: {error}"))
}

/// Only the controller owns cleanup/recovery. No TempDir or pathname-based
/// removal is allowed here. Directory fds pin writes and rename parents; the
/// controller's transaction lock must exclude other privileged publishers.
fn copy_coordinated_catalog(
    catalog: &ValidatedCatalog,
    source: &Path,
    destination: &Path,
    destination_parent: &Path,
    stage: &Path,
) -> Result<(), String> {
    if !stage.is_absolute() {
        return Err("release catalog stage must be absolute".into());
    }
    let canonical_stage = std::fs::canonicalize(stage)
        .map_err(|error| format!("resolve release catalog stage {}: {error}", stage.display()))?;
    if canonical_stage.as_os_str() != stage.as_os_str()
        || stage.parent() != Some(destination_parent)
    {
        return Err("release catalog stage must be canonical and share the destination parent".into());
    }
    if stage == source
        || stage == destination
        || stage.starts_with(source)
        || source.starts_with(stage)
        || stage.starts_with(destination)
        || destination.starts_with(stage)
    {
        return Err("release catalog stage is identical, aliased, or nested with a catalog".into());
    }
    if catalog.directory_mode & 0o022 != 0
        || catalog.manifest_mode & 0o022 != 0
        || catalog.blobs.iter().any(|blob| blob.mode & 0o022 != 0)
    {
        return Err("coordinated catalog source modes permit group/other writes".into());
    }
    let parent = open_release_directory(destination_parent)?;
    let parent_identity = Some(directory_identity(
        &parent.metadata().map_err(|error| error.to_string())?,
    ));
    let parent_view = PathBuf::from(format!("/proc/self/fd/{}", parent.as_raw_fd()));
    let stage_name = stage.file_name().ok_or("release catalog stage has no filename")?;
    let destination_name = destination
        .file_name()
        .ok_or("destination catalog has no filename")?;
    let pinned_stage = parent_view.join(stage_name);
    let pinned_destination = parent_view.join(destination_name);
    let stage_directory = open_release_directory(&pinned_stage)?;
    let stage_identity = Some(directory_identity(
        &stage_directory.metadata().map_err(|error| error.to_string())?,
    ));
    let old_identity = catalog_directory_identity(&pinned_destination)?;
    let source_identity = catalog_directory_identity(source)?;
    if stage_identity == source_identity
        || stage_identity == old_identity
        || (old_identity.is_some() && old_identity == source_identity)
    {
        return Err("release catalog stage/source/destination alias the same directory".into());
    }
    require_directory_identity(stage, stage_identity)?;
    let stage_view = PathBuf::from(format!("/proc/self/fd/{}", stage_directory.as_raw_fd()));
    if std::fs::read_dir(&stage_view)
        .map_err(|error| format!("read release catalog stage: {error}"))?
        .next()
        .transpose()
        .map_err(|error| format!("read release catalog stage entry: {error}"))?
        .is_some()
    {
        return Err("release catalog stage is not empty".into());
    }

    for blob in &catalog.blobs {
        write_release_file(&stage_directory, &stage_view, &blob.name, &blob.bytes, blob.mode)?;
    }
    write_release_file(
        &stage_directory,
        &stage_view,
        MANIFEST_NAME,
        &catalog.manifest,
        catalog.manifest_mode,
    )?;
    stage_directory
        .set_permissions(std::fs::Permissions::from_mode(catalog.directory_mode))
        .map_err(|error| format!("chmod release catalog stage: {error}"))?;
    stage_directory
        .sync_all()
        .map_err(|error| format!("sync release catalog stage: {error}"))?;
    require_directory_identity(destination_parent, parent_identity)?;
    require_directory_identity(&pinned_stage, stage_identity)?;
    require_directory_identity(&pinned_destination, old_identity)?;
    nix::fcntl::renameat2(
        Some(parent.as_raw_fd()),
        Path::new(stage_name),
        Some(parent.as_raw_fd()),
        Path::new(destination_name),
        if old_identity.is_some() {
            nix::fcntl::RenameFlags::RENAME_EXCHANGE
        } else {
            nix::fcntl::RenameFlags::RENAME_NOREPLACE
        },
    )
    .map_err(|error| format!("publish coordinated catalog: {error}; controller reconciliation required"))?;
    parent.sync_all().map_err(|error| {
        format!("sync published catalog parent: {error}; namespace changed, durability unknown; controller reconciliation required")
    })?;
    require_directory_identity(destination_parent, parent_identity)?;
    require_directory_identity(&pinned_destination, stage_identity)?;
    require_directory_identity(&pinned_stage, old_identity)?;
    Ok(())
}

/// Copy a sealed catalog as one directory transaction. Existing readers see
/// either the prior catalog or the fully validated replacement; an invalid
/// source cannot remove or rewrite destination bytes.
fn copy_catalog(src: &Path, dst: &Path) -> Result<(), String> {
    let stage = std::env::var_os("OPDBUS_RELEASE_CATALOG_STAGE").map(PathBuf::from);
    copy_catalog_with_stage(src, dst, stage.as_deref())
}

fn copy_catalog_with_stage(src: &Path, dst: &Path, stage: Option<&Path>) -> Result<(), String> {
    let catalog = validate_catalog(src)?;
    let source = std::fs::canonicalize(src)
        .map_err(|error| format!("resolve source catalog {}: {error}", src.display()))?;
    let destination_parent = dst
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let destination_name = dst
        .file_name()
        .ok_or_else(|| format!("destination catalog has no filename: {}", dst.display()))?;
    let destination_parent = std::fs::canonicalize(destination_parent).map_err(|error| {
        format!(
            "resolve destination parent {} before catalog copy: {error}",
            destination_parent.display()
        )
    })?;
    let destination = destination_parent.join(destination_name);
    if destination == source || destination.starts_with(&source) || source.starts_with(&destination)
    {
        return Err("source and destination catalogs are identical, aliased, or nested".into());
    }

    let destination_exists = match std::fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(format!(
                    "destination catalog is not a directory: {}",
                    destination.display()
                ));
            }
            let source_metadata = std::fs::metadata(&source)
                .map_err(|error| format!("inspect {}: {error}", source.display()))?;
            if metadata.dev() == source_metadata.dev() && metadata.ino() == source_metadata.ino() {
                return Err("source and destination catalogs alias the same directory".into());
            }
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "inspect destination {}: {error}",
                destination.display()
            ));
        }
    };

    if let Some(stage) = stage {
        copy_coordinated_catalog(&catalog, &source, &destination, &destination_parent, stage)
            .map_err(|error| format!(
                "{error}; coordinated paths preserved for controller reconciliation: stage={}, destination={}",
                stage.display(), destination.display()
            ))?;
        eprintln!("opblob: {} blobs + manifest", catalog.blobs.len());
        return Ok(());
    }

    let stage = tempfile::Builder::new()
        .prefix(".opblob-catalog-stage.")
        .tempdir_in(&destination_parent)
        .map_err(|error| format!("create catalog stage: {error}"))?;
    std::fs::set_permissions(
        stage.path(),
        std::fs::Permissions::from_mode(catalog.directory_mode),
    )
    .map_err(|error| format!("chmod catalog stage: {error}"))?;
    for blob in &catalog.blobs {
        write_synced(&stage.path().join(&blob.name), &blob.bytes, blob.mode)?;
    }
    write_synced(
        &stage.path().join(MANIFEST_NAME),
        &catalog.manifest,
        catalog.manifest_mode,
    )?;
    sync_directory(stage.path())?;
    validate_catalog(stage.path()).map_err(|error| format!("validate staged catalog: {error}"))?;

    if destination_exists {
        nix::fcntl::renameat2(
            None,
            stage.path(),
            None,
            &destination,
            nix::fcntl::RenameFlags::RENAME_EXCHANGE,
        )
        .map_err(|error| format!("atomically exchange catalog directory: {error}"))?;
        if let Err(error) = sync_directory(&destination_parent) {
            let rollback = nix::fcntl::renameat2(
                None,
                stage.path(),
                None,
                &destination,
                nix::fcntl::RenameFlags::RENAME_EXCHANGE,
            );
            return Err(match rollback {
                Ok(()) => match sync_directory(&destination_parent) {
                    Ok(()) => format!("{error}; restored prior catalog durably"),
                    Err(rollback_sync_error) => format!(
                        "{error}; restored prior catalog in the current namespace, but rollback durability is unknown: {rollback_sync_error}"
                    ),
                },
                Err(rollback_error) => {
                    let recovery = stage.keep();
                    format!(
                        "{error}; CRITICAL: failed to restore prior catalog: {rollback_error}; prior catalog retained at {}",
                        recovery.display()
                    )
                }
            });
        }
    } else {
        nix::fcntl::renameat2(
            None,
            stage.path(),
            None,
            &destination,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        )
        .map_err(|error| format!("atomically install catalog directory: {error}"))?;
        if let Err(error) = sync_directory(&destination_parent) {
            let rollback = std::fs::rename(&destination, stage.path());
            return Err(match rollback {
                Ok(()) => match sync_directory(&destination_parent) {
                    Ok(()) => format!("{error}; removed uncommitted catalog durably"),
                    Err(rollback_sync_error) => format!(
                        "{error}; removed the uncommitted catalog from the current namespace, but rollback durability is unknown: {rollback_sync_error}"
                    ),
                },
                Err(rollback_error) => format!(
                    "{error}; CRITICAL: failed to remove uncommitted catalog: {rollback_error}"
                ),
            });
        }
    }

    eprintln!("opblob: {} blobs + manifest", catalog.blobs.len());
    Ok(())
}

fn seal_plugins(dir: &Path) -> Result<(), String> {
    op_plugins::state_plugins::threetched::prepare_schema_capture()
        .map_err(|error| format!("capture threetched schema before sealing: {error:#}"))?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let registry = DefaultPluginRegistry::new(Arc::new(MemoryStore::new()));
    let plugins = rt
        .block_on(registry.load_all_plugins())
        .map_err(|e| e.to_string())?;

    let mut candidates = Vec::new();
    let mut missing_schema = Vec::new();
    for plugin in plugins {
        let plugin_id = plugin.name().to_string();
        // Prefer a live-probed schema (real backend reachability folded in)
        // over the static declaration; most plugins have nothing to probe
        // and schema_live() defaults to None, in which case schema() is used.
        let live = rt.block_on(plugin.schema_live());
        let schema = match live.or_else(|| plugin.schema()) {
            Some(schema) => schema,
            None => {
                missing_schema.push(plugin_id);
                continue;
            }
        };
        candidates.push(try_blobify_plugin_schema(&plugin_id, schema)?);
    }

    // Construct every candidate before opening or mutating the catalog. An
    // invalid later schema must not leave a partially replaced active catalog.
    // Publication I/O remains fallible; this is schema preflight, not a batch
    // transaction for filesystem failures.
    let mut cat = ActiveReflectionCatalog::open(dir).map_err(|e| e.to_string())?;
    let sealed = candidates.len();
    let mut sealed_ids = std::collections::HashSet::new();
    for blob in candidates {
        sealed_ids.insert(blob.manifest.plugin_id.clone());
        let path = cat.upsert_blob(&blob).map_err(|e| e.to_string())?;
        println!(
            "sealed    {}  ({} methods, schema {})",
            path.display(),
            blob.manifest.methods.len(),
            &blob.manifest.schema_hash[..16]
        );
    }

    // A blob in the catalog IS the plugin: sweep blobs whose plugin no
    // longer exists in the build (deregistration by disappearance).
    let mut swept = cat.retain_plugins(&sealed_ids).map_err(|e| e.to_string())?;
    swept.sort();
    for plugin_id in &swept {
        println!("swept     {plugin_id} (plugin no longer exists)");
    }

    println!("catalog   {}", dir.display());
    println!("hash      {}", cat.catalog_hash());
    println!("sealed    {sealed} plugin blobs");
    if !missing_schema.is_empty() {
        missing_schema.sort();
        println!("skipped   {} plugins without schema", missing_schema.len());
        for plugin_id in missing_schema {
            println!("  {plugin_id}");
        }
    }
    println!("services:");
    for s in cat.list_services() {
        println!("  {s}");
    }
    Ok(())
}

fn inspect(file: &Path) -> Result<(), String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let blob = BlobRef::new(&bytes)?;
    let m = blob.manifest()?;
    println!("plugin     {} v{}", m.plugin_id, m.schema_version);
    println!("schema     sha256 {}", m.schema_hash);
    println!(
        "dbus       {} @ {}",
        m.dbus.interface_name, m.dbus.object_path
    );
    if let Some(id) = &m.identity {
        println!(
            "identity   wg-pub {}  key-id {}",
            id.wg_public_key, id.key_id
        );
        println!(
            "session    {} ({:?})",
            id.session.session_id, id.session.lifespan
        );
    }
    for mm in &m.methods {
        println!(
            "method     {}  {}  cap={}  subid={}",
            mm.schema_name,
            mm.grpc_path,
            mm.required_capability.as_deref().unwrap_or("-"),
            mm.subid
        );
    }
    for f in descriptor::decode_file_summaries(blob.descriptor_set())? {
        println!(
            "descriptor {} pkg={} messages={:?} services={:?}",
            f.name, f.package, f.messages, f.services
        );
    }
    Ok(())
}

fn catalog(dir: &Path) -> Result<(), String> {
    let cat = ActiveReflectionCatalog::open(dir).map_err(|e| e.to_string())?;
    println!("active plugins: {:?}", cat.active_plugins());
    for s in cat.list_services() {
        println!("  {s}");
    }
    Ok(())
}

fn btrfs_seal(image: &Path, dir: &Path) -> Result<(), String> {
    let cat = ActiveReflectionCatalog::open(dir).map_err(|e| e.to_string())?;
    let paths: Vec<PathBuf> = cat.blob_paths();
    let report = btrfs::seal_blobs_into_btrfs_image(&paths, image).map_err(|e| e.to_string())?;
    println!(
        "sealed {} blobs into {} ({} bytes, subvolumes={})",
        report.blob_count,
        report.image_path.display(),
        report.image_bytes,
        report.subvolumes
    );
    println!("{}", report.mount_hint);
    Ok(())
}

fn keygen(keyfile: &Path) -> Result<(), String> {
    if keyfile.exists() {
        return Err(format!(
            "{} exists — the keypair is the account identity and its session is account-persistent; refusing to overwrite",
            keyfile.display()
        ));
    }
    let kp = WgKeypair::generate();
    kp.save(keyfile).map_err(|e| e.to_string())?;
    let id = kp.public_identity();
    println!("private key written to {}", keyfile.display());
    println!("public   {}", id.wg_public_key);
    println!("key-id   {}", id.key_id);
    println!("session  {} (account-persistent)", id.session.session_id);
    Ok(())
}

#[cfg(test)]
mod copy_catalog_tests {
    use super::*;

    fn write_catalog(directory: &Path, schemas: Vec<op_blob::PluginSchema>) {
        let mut catalog = ActiveReflectionCatalog::open(directory).unwrap();
        for schema in schemas {
            catalog
                .upsert_blob(&blob::blobify_with_identity(&schema, None))
                .unwrap();
        }
    }

    fn committed_files(directory: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut files = BTreeMap::new();
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            if name == MANIFEST_NAME || name.ends_with(".blob") {
                files.insert(name, std::fs::read(entry.path()).unwrap());
            }
        }
        files
    }

    #[test]
    fn invalid_source_leaves_prior_destination_byte_exact() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        write_catalog(&source, vec![demo::unix_socket_schema()]);
        write_catalog(&destination, vec![demo::wireguard_schema()]);
        let before = committed_files(&destination);

        let source_blob = std::fs::read_dir(&source)
            .unwrap()
            .map(Result::unwrap)
            .find(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("blob"))
            .unwrap()
            .path();
        let mut corrupt = std::fs::read(&source_blob).unwrap();
        corrupt[0] ^= 0xff;
        std::fs::write(source_blob, corrupt).unwrap();

        assert!(copy_catalog_with_stage(&source, &destination, None).is_err());
        assert_eq!(committed_files(&destination), before);
    }

    #[test]
    fn identical_and_symlink_aliased_roots_fail_without_changes() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        write_catalog(&source, vec![demo::unix_socket_schema()]);
        let before = committed_files(&source);

        assert!(copy_catalog_with_stage(&source, &source, None).is_err());
        assert_eq!(committed_files(&source), before);

        let alias = root.path().join("alias");
        symlink(&source, &alias).unwrap();
        assert!(copy_catalog_with_stage(&source, &alias, None).is_err());
        assert_eq!(committed_files(&source), before);
    }

    #[test]
    fn changed_catalog_and_removals_commit_exact_source_bytes() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        write_catalog(&source, vec![demo::unix_socket_schema()]);
        write_catalog(
            &destination,
            vec![demo::wireguard_schema(), demo::unix_socket_schema()],
        );
        let retired = committed_files(&destination)
            .into_keys()
            .find(|name| name.starts_with("wireguard."))
            .unwrap();
        let expected = committed_files(&source);

        copy_catalog_with_stage(&source, &destination, None).unwrap();

        assert_eq!(committed_files(&destination), expected);
        assert!(!destination.join(retired).exists());
    }

    #[test]
    fn coordinated_invalid_paths_preserve_every_directory() {
        let root = tempfile::tempdir().unwrap();
        let root_path = std::fs::canonicalize(root.path()).unwrap();
        let source = root_path.join("source");
        let destination = root_path.join("destination");
        let stage = root_path.join("stage");
        let alias = root_path.join("stage-alias");
        write_catalog(&source, vec![demo::unix_socket_schema()]);
        write_catalog(&destination, vec![demo::wireguard_schema()]);
        std::fs::create_dir(&stage).unwrap();
        std::os::unix::fs::symlink(&stage, &alias).unwrap();
        let before = committed_files(&destination);
        for invalid in [
            PathBuf::from("stage"),
            alias,
            root_path.join("./stage"),
            source.clone(),
            destination.clone(),
        ] {
            assert!(copy_catalog_with_stage(&source, &destination, Some(&invalid)).is_err());
            assert_eq!(committed_files(&destination), before);
            assert_eq!(std::fs::read_dir(&stage).unwrap().count(), 0);
        }
    }

    #[test]
    #[ignore = "requires root and a filesystem supporting O_TMPFILE/linkat(AT_EMPTY_PATH)"]
    fn coordinated_publish_preserves_exact_directory_identities() {
        for existing in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let root_path = std::fs::canonicalize(root.path()).unwrap();
            let source = root_path.join("source");
            let destination = root_path.join("destination");
            let stage = root_path.join("stage");
            write_catalog(&source, vec![demo::unix_socket_schema()]);
            std::fs::create_dir(&stage).unwrap();
            if existing {
                write_catalog(&destination, vec![demo::wireguard_schema()]);
            }
            let old_identity = catalog_directory_identity(&destination).unwrap();
            let stage_identity = catalog_directory_identity(&stage).unwrap();
            let old_files = existing.then(|| committed_files(&destination));

            copy_catalog_with_stage(&source, &destination, Some(&stage)).unwrap();

            assert_eq!(catalog_directory_identity(&destination).unwrap(), stage_identity);
            assert_eq!(catalog_directory_identity(&stage).unwrap(), old_identity);
            assert_eq!(committed_files(&destination), committed_files(&source));
            if let Some(old_files) = old_files {
                assert_eq!(committed_files(&stage), old_files);
            }
            assert_eq!(std::fs::read_dir(&root_path).unwrap().count(), if existing { 3 } else { 2 });
        }
    }

    #[test]
    #[ignore = "requires root"]
    fn coordinated_nonempty_stage_is_never_cleaned() {
        let root = tempfile::tempdir().unwrap();
        let root_path = std::fs::canonicalize(root.path()).unwrap();
        let source = root_path.join("source");
        let destination = root_path.join("destination");
        let stage = root_path.join("stage");
        write_catalog(&source, vec![demo::unix_socket_schema()]);
        write_catalog(&destination, vec![demo::wireguard_schema()]);
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(stage.join("foreign"), b"preserve me").unwrap();
        let stage_identity = catalog_directory_identity(&stage).unwrap();
        let before = committed_files(&destination);

        assert!(copy_catalog_with_stage(&source, &destination, Some(&stage))
            .unwrap_err().contains("not empty"));

        assert_eq!(catalog_directory_identity(&stage).unwrap(), stage_identity);
        assert_eq!(std::fs::read(stage.join("foreign")).unwrap(), b"preserve me");
        assert_eq!(committed_files(&destination), before);
    }

    #[test]
    #[ignore = "requires root and a filesystem supporting O_TMPFILE/linkat(AT_EMPTY_PATH)"]
    fn coordinated_fd_writes_do_not_follow_replaced_stage_paths() {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("stage");
        let retained = root.path().join("retained");
        std::fs::create_dir(&stage).unwrap();
        let directory = open_release_directory(&stage).unwrap();
        let identity = catalog_directory_identity(&stage).unwrap();
        let view = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
        std::fs::rename(&stage, &retained).unwrap();
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(stage.join("foreign"), b"preserve me").unwrap();

        write_release_file(&directory, &view, "declared.blob", b"complete bytes", 0o644).unwrap();
        assert!(write_release_file(&directory, &view, "declared.blob", b"replacement", 0o644).is_err());
        assert!(require_directory_identity(&stage, identity).is_err());
        drop(directory);

        assert_eq!(std::fs::read(retained.join("declared.blob")).unwrap(), b"complete bytes");
        assert_eq!(std::fs::read_dir(&retained).unwrap().count(), 1);
        assert_eq!(std::fs::read(stage.join("foreign")).unwrap(), b"preserve me");
        assert_eq!(std::fs::read_dir(&stage).unwrap().count(), 1);
    }
}
