//! Production lifecycle: every required owner binds before readiness is published.
use std::{future::Future, io::Write as _, net::SocketAddr, os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _}, path::PathBuf, sync::Arc};
use anyhow::{Context as _, ensure};
use tokio::{sync::watch, task::JoinSet};
use crate::{Catalog, runtime::Runtime, service::BridgeService, tls_door::PemIdentity};

struct ReadyReceipt { path: PathBuf, pid: u32, start: u64 }
impl ReadyReceipt {
    fn publish(listen: SocketAddr, lungs: &std::path::Path, exhook: SocketAddr) -> anyhow::Result<Self> {
        ensure!(nix::unistd::geteuid().is_root(), "production bridge requires its root-owned service identity");
        let path = std::env::var_os("OPDBUS_READY_PATH").map(PathBuf::from)
            .unwrap_or_else(|| "/run/opdbus/runit-ready/op-grpc-bridge".into());
        let parent = path.parent().context("ready receipt has no parent")?;
        let metadata = std::fs::symlink_metadata(parent)?;
        ensure!(metadata.is_dir() && metadata.uid() == 0 && metadata.permissions().mode() & 0o022 == 0,
            "ready receipt directory is not protected");
        let stat = std::fs::read_to_string("/proc/self/stat")?;
        let fields = stat.rsplit_once(") ").context("invalid process identity record")?.1;
        let start = fields.split_ascii_whitespace().nth(19).context("process start identity missing")?.parse::<u64>()?;
        let pid = std::process::id();
        let temporary = parent.join(format!(".bridge-ready-{pid}-{}", uuid::Uuid::new_v4().simple()));
        let result = (|| -> anyhow::Result<()> {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
                .custom_flags(nix::libc::O_NOFOLLOW).open(&temporary)?;
            serde_json::to_writer(&mut file, &serde_json::json!({
                "schema":"opdbus.bridge-ready.v1", "pid":pid, "start_time_ticks":start,
                "listen_addr":listen.to_string(), "transport":"tls", "runtime_ready":true,
                "lungs_ready":true, "lungs_socket":lungs, "exhook_ready":true,
                "exhook_addr":exhook.to_string(),
            }))?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)?;
            Ok(())
        })();
        if result.is_err() { let _ = std::fs::remove_file(&temporary); }
        result?;
        Ok(Self { path, pid, start })
    }
}
impl Drop for ReadyReceipt {
    fn drop(&mut self) {
        // Never erase a successor's receipt during an overlapping shutdown.
        if let Ok(file) = std::fs::OpenOptions::new().read(true).custom_flags(nix::libc::O_NOFOLLOW).open(&self.path) {
            if let Ok(value) = serde_json::from_reader::<_, serde_json::Value>(file) {
                if value["pid"].as_u64() == Some(u64::from(self.pid)) && value["start_time_ticks"].as_u64() == Some(self.start) {
                    let _ = std::fs::remove_file(&self.path);
                }
            }
        }
    }
}

async fn stopping(mut stop: watch::Receiver<bool>) {
    if *stop.borrow() { return; }
    let _ = stop.changed().await;
}

fn spawn_owner<F>(workers: &mut JoinSet<anyhow::Result<()>>, name: &'static str, work: F)
where F: Future<Output = anyhow::Result<()>> + Send + 'static {
    workers.spawn(async move { work.await.with_context(|| format!("{name} owner stopped")) });
}

pub async fn run_from_env() -> anyhow::Result<()> {
    let addr: SocketAddr = std::env::var("OP_GRPC_BRIDGE_LISTEN")
        .unwrap_or_else(|_| "10.0.0.2:8090".into()).parse()?;
    ensure!(addr.port() == 8090 && !addr.ip().is_unspecified(), "production door requires its explicit TLS service address");
    let catalog_dir = std::env::var_os("OP_BLOB_CATALOG_DIR").map(PathBuf::from)
        .unwrap_or_else(|| op_blob::catalog::DEFAULT_SHM_DIR.into());
    let catalog = Catalog::from_sealed(&catalog_dir)?;
    let database = std::env::var_os("OP_BRIDGE_CORE_DB_PATH").map(PathBuf::from)
        .unwrap_or_else(|| "/var/lib/op-dbus/bridge-core".into());
    let oracle = Arc::new(crate::oracle_assertion::AssertionValidator::from_env(
        crate::oracle_assertion::DecoyTrustStore::load(),
    ).context("initialize durable oracle replay admission")?);
    let runtime = Arc::new(Runtime::open(catalog, catalog_dir, database).await?);
    let cert = std::env::var_os("THREETCHED_TLS_CERT_FILE").map(PathBuf::from)
        .unwrap_or_else(|| "/etc/op-dbus/tls/tonic-svc0.crt".into());
    let key = std::env::var_os("THREETCHED_TLS_KEY_FILE").map(PathBuf::from)
        .unwrap_or_else(|| "/etc/op-dbus/tls/tonic-svc0.key".into());
    let service = BridgeService::production(runtime.clone(), oracle.clone());
    let door = crate::server::bind_production(addr, service, PemIdentity::from_root_owned_files(cert, key)?).await?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut lungs = crate::runtime_lungs::start(runtime.clone()).await?;
    let (stop, receive) = watch::channel(false);
    let mut workers = JoinSet::new();
    let mut ready = None;
    // Every failure after lungs starts accepting uses the same bounded teardown.
    let result = async {
        let exhook = crate::runtime_mqtt::ExHookServer::bind_from_env(runtime.clone(), oracle).await?;
        let wake = exhook.wake_publisher();
        ready = Some(ReadyReceipt::publish(door.local_addr(), lungs.socket_path(), exhook.local_addr())?);
        spawn_owner(&mut workers, "TLS door", door.serve(stopping(receive.clone())));
        spawn_owner(&mut workers, "private ExHook", exhook.serve(stopping(receive.clone())));
        spawn_owner(&mut workers, "committed wake", wake.run(runtime.core().clone(), stopping(receive.clone())));
        let observed_runtime = runtime.as_ref().clone();
        let observer_stop = receive.clone();
        spawn_owner(&mut workers, "native network observations", async move {
            tokio::select! {
                result = crate::runtime_network::NetworkRuntime::observe(observed_runtime) => result,
                _ = stopping(observer_stop) => Ok(()),
            }
        });
        tracing::info!(%addr, plugins = runtime.catalog().plugins().len(), "authenticated bridge owners are ready");
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.map_err(anyhow::Error::from),
            _ = terminate.recv() => Ok(()),
            result = lungs.terminated() => result.and_then(|_| anyhow::bail!("lungs owner exited unexpectedly")),
            result = workers.join_next() => match result {
                Some(Ok(Err(error))) => Err(error),
                Some(Err(error)) => Err(error.into()),
                _ => Err(anyhow::anyhow!("required bridge owner exited unexpectedly")),
            },
        }
    }.await;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(35);
    runtime.begin_shutdown();
    lungs.begin_shutdown();
    drop(ready);
    stop.send_replace(true);
    let drained = tokio::time::timeout_at(deadline, async {
        let native = async {
            // An admitted effect must retain its native owner until it finishes.
            // A deadline leaves its durable attempt unresolved, never cancelled.
            runtime.drain_admitted().await;
            runtime.network().shutdown().await;
        };
        let owners = async {
            let mut failure = None;
            while let Some(result) = workers.join_next().await {
                let result = result.map_err(anyhow::Error::from).and_then(|result| result);
                if let Err(error) = result {
                    tracing::warn!(%error, "bridge owner failed during shutdown");
                    if failure.is_none() { failure = Some(error); }
                }
            }
            failure.map_or(Ok(()), Err)
        };
        // Neither an owner error nor a stuck network teardown may skip drainage
        // of the other transports or move it outside the overall deadline.
        let (lungs_result, owners_result, ()) = tokio::join!(lungs.shutdown(), owners, native);
        lungs_result?;
        owners_result
    }).await;
    let shutdown = match drained {
        Ok(result) => result,
        Err(error) => {
            workers.abort_all();
            Err(anyhow::Error::from(error).context(
                "bridge shutdown deadline exceeded; unfinished native effects remain outcome-unknown",
            ))
        }
    };
    match (result, shutdown) {
        (Err(error), Err(shutdown)) => Err(error.context(format!("bridge shutdown also failed: {shutdown:#}"))),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), shutdown) => shutdown,
    }
}
