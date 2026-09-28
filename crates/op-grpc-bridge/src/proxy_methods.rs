//! MutationEngine-owned, event-driven fixed-destination TCP relays.
//! Cozo owns configuration; live listener handles own running state. A restart
//! retains configuration and requires an explicit start, never a fabricated
//! persisted `running` flag. No subprocess or independent control listener.
use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::{
    proxy_server::{
        ConfigureProxyInput, ProxyConfig, ProxyMethodOutput, ProxyServerState, ProxyStatus,
        StartProxyInput, StopProxyInput,
    },
    runtime_documents::RuntimeDocuments,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{Mutex, Notify, oneshot, watch},
    task::{JoinHandle, JoinSet},
};

const PLUGIN: &str = "proxy_server";
const MAX_PROXIES: usize = 64;
const MAX_CONNECTIONS: usize = 256;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Configurations {
    proxies: BTreeMap<String, ProxyConfig>,
    #[serde(default)]
    last_request: Option<(String, Value)>,
}

struct Listener {
    bound: SocketAddr,
    upstream: watch::Sender<SocketAddr>,
    task: JoinHandle<()>,
    shutdown: Option<oneshot::Sender<()>>,
    running: Arc<AtomicBool>,
}
impl Listener {
    async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = (&mut self.task).await;
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(crate) struct ProxyMethods {
    documents: Arc<RuntimeDocuments>,
    listeners: Mutex<BTreeMap<String, Listener>>,
    pub(crate) changed: Arc<Notify>,
}

impl ProxyMethods {
    pub(crate) fn new(documents: Arc<RuntimeDocuments>) -> Self {
        Self {
            documents,
            listeners: Mutex::new(BTreeMap::new()),
            changed: Arc::new(Notify::new()),
        }
    }

    pub(crate) async fn snapshot(&self) -> Result<ProxyServerState> {
        let listeners = self.listeners.lock().await;
        let row = self.documents.read_durable(PLUGIN).await?;
        let revision = row.as_ref().map_or(0, |row| row.revision);
        let configs = row
            .map(|row| serde_json::from_value(row.document))
            .transpose()?
            .unwrap_or_default();
        Ok(snapshot(revision, configs, &listeners))
    }

    /// Caller holds the engine's operation lock through SHM publication.
    /// The enclosing owned method task survives transport cancellation.
    pub(crate) async fn dispatch(
        &self,
        method: &str,
        args: Value,
    ) -> Result<(Value, ProxyServerState)> {
        let mut listeners = self.listeners.lock().await;
        let previous = self.documents.read_durable(PLUGIN).await?;
        let revision = previous.as_ref().map_or(0, |row| row.revision);
        let mut configs: Configurations = previous
            .map(|row| serde_json::from_value(row.document))
            .transpose()?
            .unwrap_or_default();
        let (name, expected) = match method {
            "configure" => {
                let input: ConfigureProxyInput = serde_json::from_value(args.clone())?;
                (input.name, input.expected_revision)
            }
            "start_proxy" => {
                let input: StartProxyInput = serde_json::from_value(args.clone())?;
                (input.name, input.expected_revision)
            }
            "stop_proxy" => {
                let input: StopProxyInput = serde_json::from_value(args.clone())?;
                (input.name, input.expected_revision)
            }
            _ => anyhow::bail!("unknown proxy method"),
        };
        validate_name(&name)?;
        let retry = expected.checked_add(1) == Some(revision)
            && configs.last_request.as_ref().is_some_and(
                |(previous_method, previous_arguments)| {
                    previous_method == method && *previous_arguments == args
                },
            );
        ensure!(
            revision == expected || retry,
            "proxy revision conflict: current revision is {revision}"
        );
        let request = args.clone();
        let mut prepared = None;
        match method {
            "configure" => {
                let input: ConfigureProxyInput = serde_json::from_value(args)?;
                let old = configs.proxies.get(&name).cloned();
                ensure!(
                    old.is_some() || configs.proxies.len() < MAX_PROXIES,
                    "proxy count limit reached"
                );
                let config = ProxyConfig {
                    enabled: input
                        .enabled
                        .or_else(|| old.as_ref().map(|v| v.enabled))
                        .unwrap_or(false),
                    bind_address: input
                        .bind_address
                        .or_else(|| old.as_ref().map(|v| v.bind_address.clone()))
                        .unwrap_or_else(|| "127.0.0.1".into()),
                    port: input
                        .port
                        .or_else(|| old.as_ref().map(|v| v.port))
                        .context("new proxy requires port")?,
                    upstream: input
                        .upstream
                        .or_else(|| old.as_ref().map(|v| v.upstream.clone()))
                        .context("new proxy requires upstream")?,
                };
                let (bind, _) = addresses(&config)?;
                if config.enabled
                    && listeners
                        .get(&name)
                        .is_some_and(|v| v.running.load(Ordering::Acquire) && v.bound != bind)
                {
                    prepared = Some(
                        TcpListener::bind(bind)
                            .await
                            .context("replacement proxy bind failed")?,
                    );
                }
                configs.proxies.insert(name.clone(), config);
            }
            "start_proxy" => {
                let input: StartProxyInput = serde_json::from_value(args)?;
                let config = configs
                    .proxies
                    .get_mut(&name)
                    .context("configure proxy before starting")?;
                ensure!(config.enabled, "proxy is disabled");
                if let Some(port) = input.port {
                    config.port = port;
                }
                let (bind, _) = addresses(config)?;
                if !listeners
                    .get(&name)
                    .is_some_and(|v| v.running.load(Ordering::Acquire) && v.bound == bind)
                {
                    prepared = Some(TcpListener::bind(bind).await.context("proxy bind failed")?);
                }
            }
            "stop_proxy" => {
                ensure!(configs.proxies.contains_key(&name), "unknown proxy");
            }
            _ => unreachable!(),
        }
        // Even lifecycle transitions advance the CAS revision. Binding failure
        // or CAS conflict leaves the previous relay accepting unchanged.
        configs.last_request = Some((method.into(), request));
        let revision = if retry {
            revision
        } else {
            let committed = serde_json::to_value(&configs)?;
            self.documents
                .update_at_revision_with_result(PLUGIN, expected, move |_| Ok((committed, ())))
                .await?
                .0
                .revision
        };
        let config = configs
            .proxies
            .get(&name)
            .context("missing configured proxy")?;
        let (_, upstream) = addresses(config)?;
        if method == "stop_proxy" || !config.enabled {
            if let Some(listener) = listeners.remove(&name) {
                listener.stop().await;
            }
        } else if let Some(listener) = prepared {
            let bound = listener.local_addr()?;
            let (sender, receiver) = watch::channel(upstream);
            let (shutdown, stopped) = oneshot::channel();
            let running = Arc::new(AtomicBool::new(true));
            let task_running = running.clone();
            let changed = self.changed.clone();
            let task = tokio::spawn(async move {
                serve(listener, receiver, stopped).await;
                // Publish terminal status before the wake-up. JoinHandle
                // completion happens later and is not our observation flag.
                task_running.store(false, Ordering::Release);
                changed.notify_one();
            });
            if let Some(old) = listeners.insert(
                name.clone(),
                Listener {
                    bound,
                    upstream: sender,
                    task,
                    shutdown: Some(shutdown),
                    running,
                },
            ) {
                old.stop().await;
            }
        } else if let Some(listener) = listeners.get(&name) {
            listener.upstream.send_replace(upstream);
        }
        let state = snapshot(revision, configs, &listeners);
        let output = ProxyMethodOutput {
            name: name.clone(),
            revision,
            status: state.proxies[&name].clone(),
        };
        Ok((serde_json::to_value(output)?, state))
    }
}

fn snapshot(
    revision: u64,
    configs: Configurations,
    listeners: &BTreeMap<String, Listener>,
) -> ProxyServerState {
    ProxyServerState {
        revision,
        proxies: configs
            .proxies
            .into_iter()
            .map(|(name, config)| {
                let listener = listeners
                    .get(&name)
                    .filter(|listener| listener.running.load(Ordering::Acquire));
                let status = ProxyStatus {
                    config,
                    running: listener.is_some(),
                    bound_address: listener.map(|v| v.bound.to_string()),
                };
                (name, status)
            })
            .collect(),
    }
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "invalid proxy name"
    );
    Ok(())
}

fn addresses(config: &ProxyConfig) -> Result<(SocketAddr, SocketAddr)> {
    let ip: IpAddr = config
        .bind_address
        .parse()
        .context("proxy bind requires a literal IP")?;
    ensure!(
        config.port != 0 && !ip.is_multicast(),
        "invalid proxy bind address or port"
    );
    let upstream: SocketAddr = config
        .upstream
        .parse()
        .context("proxy upstream requires literal IP:port")?;
    ensure!(
        upstream.port() != 0 && !upstream.ip().is_unspecified() && !upstream.ip().is_multicast(),
        "invalid proxy upstream"
    );
    let bind = SocketAddr::new(ip, config.port);
    ensure!(
        bind != upstream && !(bind.ip().is_unspecified() && bind.port() == upstream.port()),
        "proxy cannot forward to itself"
    );
    Ok((bind, upstream))
}

async fn serve(
    listener: TcpListener,
    upstream: watch::Receiver<SocketAddr>,
    mut shutdown: oneshot::Receiver<()>,
) {
    // JoinSet drop aborts every relay when the listener owner stops/rebinds.
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            result = listener.accept(), if connections.len() < MAX_CONNECTIONS => {
                let Ok((mut incoming, _)) = result else { break };
                let destination = *upstream.borrow();
                connections.spawn(async move {
                    let Ok(Ok(mut outgoing)) = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(destination)).await else { return };
                    // Fixed-duration bound also releases idle/abandoned peers.
                    let _ = tokio::time::timeout(Duration::from_secs(3600), tokio::io::copy_bidirectional(&mut incoming, &mut outgoing)).await;
                });
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
    drop(listener);
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn setup() -> (ProxyMethods, Arc<RuntimeDocuments>) {
        let docs = Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        (ProxyMethods::new(docs.clone()), docs)
    }
    async fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }
    async fn echo() -> (SocketAddr, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let (mut read, mut write) = stream.into_split();
                    let _ = tokio::io::copy(&mut read, &mut write).await;
                });
            }
        });
        (address, task)
    }

    #[tokio::test]
    async fn configure_start_bytes_reconfigure_and_stop_have_real_effects() {
        let (service, _) = setup().await;
        let (upstream, echo_task) = echo().await;
        let port = free_port().await;
        let (_, state) = service.dispatch("configure", serde_json::json!({"name":"fixture", "enabled":true,"port":port,"upstream":upstream.to_string(),"expected_revision":0})).await.unwrap();
        assert!(!state.proxies["fixture"].running);
        assert!(
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .is_err()
        );
        let (_, state) = service
            .dispatch(
                "start_proxy",
                serde_json::json!({"name":"fixture","expected_revision":1}),
            )
            .await
            .unwrap();
        assert!(state.proxies["fixture"].running);
        let mut client = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        client.write_all(b"through-real-upstream").await.unwrap();
        let mut response = vec![0; 21];
        tokio::time::timeout(Duration::from_secs(3), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response, b"through-real-upstream");
        assert!(
            service
                .dispatch(
                    "stop_proxy",
                    serde_json::json!({"name":"fixture","expected_revision":1})
                )
                .await
                .is_err()
        );
        let replacement = free_port().await;
        let (_, state) = service
            .dispatch(
                "configure",
                serde_json::json!({"name":"fixture","port":replacement,"expected_revision":2}),
            )
            .await
            .unwrap();
        assert!(state.proxies["fixture"].running);
        assert!(
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .is_err()
        );
        let (_, state) = service
            .dispatch(
                "stop_proxy",
                serde_json::json!({"name":"fixture","expected_revision":3}),
            )
            .await
            .unwrap();
        assert!(!state.proxies["fixture"].running);
        assert!(
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, replacement))
                .await
                .is_err()
        );
        echo_task.abort();
    }

    #[tokio::test]
    async fn exact_start_retry_recovers_after_owner_restart_without_consuming_revision() {
        let (service, documents) = setup().await;
        let port = free_port().await;
        service.dispatch("configure", serde_json::json!({"name":"fixture","enabled":true,"port":port,"upstream":"127.0.0.1:1","expected_revision":0})).await.unwrap();
        let request = serde_json::json!({"name":"fixture","expected_revision":1});
        service
            .dispatch("start_proxy", request.clone())
            .await
            .unwrap();
        drop(service);
        // Wait for task cancellation from the dead runtime owner, without
        // treating a persisted revision as evidence of a live listener.
        tokio::task::yield_now().await;
        let restarted = ProxyMethods::new(documents);
        assert!(!restarted.snapshot().await.unwrap().proxies["fixture"].running);
        let (reply, state) = restarted
            .dispatch("start_proxy", request.clone())
            .await
            .unwrap();
        assert!(state.proxies["fixture"].running);
        assert_eq!(state.revision, 2);
        let (retry, _) = restarted.dispatch("start_proxy", request).await.unwrap();
        assert_eq!(retry, reply);
        restarted
            .dispatch(
                "stop_proxy",
                serde_json::json!({"name":"fixture","expected_revision":2}),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn failed_bind_preserves_config_and_restart_does_not_claim_running() {
        let (service, documents) = setup().await;
        let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = occupied.local_addr().unwrap().port();
        service.dispatch("configure", serde_json::json!({"name":"fixture","enabled":true,"port":port,"upstream":"127.0.0.1:1","expected_revision":0})).await.unwrap();
        assert!(
            service
                .dispatch(
                    "start_proxy",
                    serde_json::json!({"name":"fixture","expected_revision":1})
                )
                .await
                .is_err()
        );
        assert_eq!(documents.read(PLUGIN).await.unwrap().unwrap().revision, 1);
        drop(occupied);
        service
            .dispatch(
                "start_proxy",
                serde_json::json!({"name":"fixture","expected_revision":1}),
            )
            .await
            .unwrap();
        drop(service);
        let reopened = ProxyMethods::new(documents);
        let (_, state) = reopened
            .dispatch(
                "configure",
                serde_json::json!({"name":"fixture","expected_revision":2}),
            )
            .await
            .unwrap();
        assert!(!state.proxies["fixture"].running);
    }
}
