//! Native wire mappings for the plugin's login1 contract.
//! Signature source: systemd/man/org.freedesktop.login1.xml (also used by elogind).
use super::*;
use anyhow::{bail, ensure, Context};
use serde::de::DeserializeOwned;
use serde_json::{to_value, Value as Json};
use std::collections::BTreeMap;
use std::sync::OnceLock;
use tokio::sync::Mutex;
use zbus::zvariant::{OwnedFd, OwnedValue, Value as DbusValue};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UserInfo {
    pub uid: u32,
    pub user: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SeatInfo {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListSessionsOutput {
    pub sessions: Vec<SessionInfo>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListUsersOutput {
    pub users: Vec<UserInfo>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListSeatsOutput {
    pub seats: Vec<SeatInfo>,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetObjectOutput {
    pub object_path: String,
}

/// A successful empty D-Bus reply means the operation was accepted. It does
/// not mean a power transition finished or a screen-lock handler responded.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LoginOperationOutput {
    pub operation: String,
    pub disposition: LoginOperationDisposition,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LoginOperationDisposition {
    Accepted,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateSessionOutput {
    pub session_id: String,
    pub object_path: String,
    pub runtime_path: String,
    pub uid: u32,
    pub seat_id: String,
    pub vtnr: u32,
    pub existing: bool,
}

/// Typed transient-scope property values. D-Bus variants retain their width;
/// JSON numbers alone must never choose a D-Bus signature implicitly.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum SessionPropertyValue {
    Bool(bool),
    Byte(u8),
    Int32(i32),
    Uint32(u32),
    Int64(i64),
    Uint64(u64),
    Double(f64),
    String(String),
    Strings(Vec<String>),
    Bytes(Vec<u8>),
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionProperty {
    pub name: String,
    pub value: SessionPropertyValue,
}
impl SessionPropertyValue {
    fn into_dbus(self) -> Result<OwnedValue> {
        Ok(match self {
            Self::Bool(v) => v.into(),
            Self::Byte(v) => v.into(),
            Self::Int32(v) => v.into(),
            Self::Uint32(v) => v.into(),
            Self::Int64(v) => v.into(),
            Self::Uint64(v) => v.into(),
            Self::Double(v) => {
                ensure!(v.is_finite(), "property must be finite");
                v.into()
            }
            Self::String(v) => DbusValue::from(v).try_into()?,
            Self::Strings(v) => DbusValue::from(v).try_into()?,
            Self::Bytes(v) => DbusValue::from(v).try_into()?,
        })
    }
}

/// CreateSession transfers a lifetime FIFO, not a portable integer file
/// descriptor. Hold it until ReleaseSession/TerminateSession or process exit.
/// These are login1 leases, completely separate from OP-DBUS SID1 identities.
struct SessionLease {
    uid: u32,
    seat: String,
    _fifo: OwnedFd,
}
#[derive(Default)]
struct SessionLeases(Mutex<BTreeMap<String, SessionLease>>);
static SESSION_LEASES: OnceLock<SessionLeases> = OnceLock::new();

fn input<T: DeserializeOwned>(args: &Json) -> Result<T> {
    serde_json::from_value(args.clone()).context("invalid login1 method input")
}

pub async fn dispatch_login1_method(method: &str, args: &Json) -> Result<Json> {
    let proxy = Login1Plugin::new().connect_manager().await?;
    dispatch_with_proxy(
        &proxy,
        SESSION_LEASES.get_or_init(Default::default),
        method,
        args,
    )
    .await
}

async fn dispatch_with_proxy(
    proxy: &Proxy<'_>,
    leases: &SessionLeases,
    method: &str,
    args: &Json,
) -> Result<Json> {
    // Also serializes lease creation/release so an overlapping release cannot
    // run between the upstream create and our acquisition of its FIFO.
    let mut held = leases.0.lock().await;
    match method {
        "list_sessions" => {
            let rows: Vec<(String, u32, String, String, OwnedObjectPath)> =
                proxy.call("ListSessions", &()).await?;
            to_value(ListSessionsOutput {
                sessions: rows
                    .into_iter()
                    .map(|(id, uid, user, seat, path)| SessionInfo {
                        id,
                        uid,
                        user,
                        seat,
                        path: path.to_string(),
                    })
                    .collect(),
            })
            .map_err(Into::into)
        }
        "list_users" => {
            let rows: Vec<(u32, String, OwnedObjectPath)> = proxy.call("ListUsers", &()).await?;
            Ok(to_value(ListUsersOutput {
                users: rows
                    .into_iter()
                    .map(|(uid, user, path)| UserInfo {
                        uid,
                        user,
                        path: path.to_string(),
                    })
                    .collect(),
            })?)
        }
        "list_seats" => {
            let rows: Vec<(String, OwnedObjectPath)> = proxy.call("ListSeats", &()).await?;
            Ok(to_value(ListSeatsOutput {
                seats: rows
                    .into_iter()
                    .map(|(id, path)| SeatInfo {
                        id,
                        path: path.to_string(),
                    })
                    .collect(),
            })?)
        }
        "get_session" | "get_user" | "get_seat" => {
            let path: OwnedObjectPath = match method {
                "get_session" => {
                    proxy
                        .call("GetSession", &(input::<GetSessionInput>(args)?.session_id,))
                        .await?
                }
                "get_user" => {
                    proxy
                        .call("GetUser", &(input::<GetUserInput>(args)?.uid,))
                        .await?
                }
                _ => {
                    proxy
                        .call("GetSeat", &(input::<GetSeatInput>(args)?.seat_id,))
                        .await?
                }
            };
            Ok(to_value(GetObjectOutput {
                object_path: path.to_string(),
            })?)
        }
        "create_session" => {
            let i: CreateSessionInput = input(args)?;
            let properties: Vec<(String, OwnedValue)> = i
                .properties
                .into_iter()
                .map(|p| Ok((p.name, p.value.into_dbus()?)))
                .collect::<Result<_>>()?;
            let (session_id, path, runtime_path, fd, uid, seat_id, vtnr, existing): (
                String,
                OwnedObjectPath,
                String,
                OwnedFd,
                u32,
                String,
                u32,
                bool,
            ) = proxy
                .call(
                    "CreateSession",
                    &(
                        i.uid,
                        i.pid,
                        i.service,
                        i.type_,
                        i.class,
                        i.desktop,
                        i.seat_id,
                        i.vtnr,
                        i.tty,
                        i.display,
                        i.remote,
                        i.remote_user,
                        i.remote_host,
                        properties,
                    ),
                )
                .await?;
            // Keep one FIFO per session: repeated creates must not accumulate
            // descriptors for an already registered login session.
            held.insert(
                session_id.clone(),
                SessionLease {
                    uid,
                    seat: seat_id.clone(),
                    _fifo: fd,
                },
            );
            Ok(to_value(CreateSessionOutput {
                session_id,
                object_path: path.to_string(),
                runtime_path,
                uid,
                seat_id,
                vtnr,
                existing,
            })?)
        }
        "release_session" | "terminate_session" | "lock_session" | "unlock_session" => {
            let i: SessionIdInput = input(args)?;
            let upstream = match method {
                "release_session" => "ReleaseSession",
                "terminate_session" => "TerminateSession",
                "lock_session" => "LockSession",
                _ => "UnlockSession",
            };
            proxy
                .call::<_, _, ()>(upstream, &(i.session_id.clone(),))
                .await?;
            if matches!(method, "release_session" | "terminate_session") {
                held.remove(&i.session_id);
            }
            accepted(method)
        }
        "kill_session" => {
            let i: KillSessionInput = input(args)?;
            ensure!(
                matches!(i.who.as_str(), "all" | "leader"),
                "who must be all or leader"
            );
            proxy
                .call::<_, _, ()>("KillSession", &(i.session_id, i.who, i.signal_number))
                .await?;
            accepted(method)
        }
        "terminate_user" => {
            let uid = input::<UidInput>(args)?.uid;
            proxy.call::<_, _, ()>("TerminateUser", &(uid,)).await?;
            held.retain(|_, lease| lease.uid != uid);
            accepted(method)
        }
        "kill_user" => {
            let i: KillUserInput = input(args)?;
            proxy
                .call::<_, _, ()>("KillUser", &(i.uid, i.signal_number))
                .await?;
            accepted(method)
        }
        "set_user_linger" => {
            let i: SetUserLingerInput = input(args)?;
            proxy
                .call::<_, _, ()>("SetUserLinger", &(i.uid, i.enable, i.interactive))
                .await?;
            accepted(method)
        }
        "terminate_seat" => {
            let seat = input::<SeatIdInput>(args)?.seat_id;
            proxy.call::<_, _, ()>("TerminateSeat", &(&seat,)).await?;
            held.retain(|_, lease| lease.seat != seat);
            accepted(method)
        }
        "attach_device" => {
            let i: AttachDeviceInput = input(args)?;
            proxy
                .call::<_, _, ()>("AttachDevice", &(i.seat_id, i.sysfs_path, i.interactive))
                .await?;
            accepted(method)
        }
        "power_off" | "reboot" | "suspend" | "hibernate" | "flush_devices" => {
            let i: InteractiveInput = input(args)?;
            let upstream = match method {
                "power_off" => "PowerOff",
                "reboot" => "Reboot",
                "suspend" => "Suspend",
                "hibernate" => "Hibernate",
                _ => "FlushDevices",
            };
            proxy.call::<_, _, ()>(upstream, &(i.interactive,)).await?;
            accepted(method)
        }
        "sleep" => {
            proxy
                .call::<_, _, ()>("Sleep", &(input::<SleepInput>(args)?.flags,))
                .await?;
            accepted(method)
        }
        _ => bail!("unknown login1 method '{method}'"),
    }
}

fn accepted(method: &str) -> Result<Json> {
    Ok(to_value(LoginOperationOutput {
        operation: method.to_string(),
        disposition: LoginOperationDisposition::Accepted,
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::fd::AsFd;
    use std::sync::{Arc, Mutex as StdMutex};
    use zbus::{connection::Builder, Guid};

    type Calls = Arc<StdMutex<Vec<(String, Json)>>>;
    struct Manager {
        calls: Calls,
        fifo: OwnedFd,
    }
    impl Manager {
        fn record(&self, method: &str, args: Json) {
            self.calls.lock().unwrap().push((method.into(), args));
        }
    }
    #[zbus::interface(name = "org.freedesktop.login1.Manager")]
    impl Manager {
        fn list_sessions(&self) -> Vec<(String, u32, String, String, OwnedObjectPath)> {
            vec![(
                "s7".into(),
                1001,
                "fixture".into(),
                "seat0".into(),
                OwnedObjectPath::try_from("/session/s7").unwrap(),
            )]
        }
        fn list_users(&self) -> Vec<(u32, String, OwnedObjectPath)> {
            vec![(
                1001,
                "fixture".into(),
                OwnedObjectPath::try_from("/user/u1001").unwrap(),
            )]
        }
        fn list_seats(&self) -> Vec<(String, OwnedObjectPath)> {
            vec![(
                "seat0".into(),
                OwnedObjectPath::try_from("/seat/seat0").unwrap(),
            )]
        }
        fn get_session(&self, id: &str) -> OwnedObjectPath {
            self.record("GetSession", json!(id));
            OwnedObjectPath::try_from("/session/s7").unwrap()
        }
        fn get_user(&self, uid: u32) -> OwnedObjectPath {
            self.record("GetUser", json!(uid));
            OwnedObjectPath::try_from("/user/u1001").unwrap()
        }
        fn get_seat(&self, id: &str) -> zbus::fdo::Result<OwnedObjectPath> {
            if id == "absent" {
                return Err(zbus::fdo::Error::Failed("fixture missing seat".into()));
            }
            self.record("GetSeat", json!(id));
            Ok(OwnedObjectPath::try_from("/seat/seat0").unwrap())
        }
        #[allow(clippy::too_many_arguments)]
        fn create_session(
            &self,
            uid: u32,
            pid: u32,
            service: &str,
            type_: &str,
            class: &str,
            desktop: &str,
            seat_id: &str,
            vtnr: u32,
            tty: &str,
            display: &str,
            remote: bool,
            remote_user: &str,
            remote_host: &str,
            properties: Vec<(String, OwnedValue)>,
        ) -> (
            String,
            OwnedObjectPath,
            String,
            OwnedFd,
            u32,
            String,
            u32,
            bool,
        ) {
            self.record(
                "CreateSession",
                json!([
                    uid,
                    pid,
                    service,
                    type_,
                    class,
                    desktop,
                    seat_id,
                    vtnr,
                    tty,
                    display,
                    remote,
                    remote_user,
                    remote_host,
                    properties.len()
                ]),
            );
            assert_eq!(properties[0].0, "MemoryMax");
            assert_eq!(
                u64::try_from(properties[0].1.try_clone().unwrap()).unwrap(),
                8192
            );
            (
                "s7".into(),
                OwnedObjectPath::try_from("/session/s7").unwrap(),
                "/runtime/1001".into(),
                self.fifo.as_fd().try_clone_to_owned().unwrap().into(),
                uid,
                seat_id.into(),
                vtnr,
                false,
            )
        }
        fn release_session(&self, id: &str) {
            self.record("ReleaseSession", json!(id));
        }
        fn terminate_session(&self, id: &str) {
            self.record("TerminateSession", json!(id));
        }
        fn lock_session(&self, id: &str) {
            self.record("LockSession", json!(id));
        }
        fn unlock_session(&self, id: &str) {
            self.record("UnlockSession", json!(id));
        }
        fn kill_session(&self, id: &str, who: &str, signal: i32) {
            self.record("KillSession", json!([id, who, signal]));
        }
        fn terminate_user(&self, uid: u32) {
            self.record("TerminateUser", json!(uid));
        }
        fn kill_user(&self, uid: u32, signal: i32) {
            self.record("KillUser", json!([uid, signal]));
        }
        fn set_user_linger(&self, uid: u32, enable: bool, interactive: bool) {
            self.record("SetUserLinger", json!([uid, enable, interactive]));
        }
        fn terminate_seat(&self, id: &str) {
            self.record("TerminateSeat", json!(id));
        }
        fn attach_device(&self, id: &str, path: &str, interactive: bool) {
            self.record("AttachDevice", json!([id, path, interactive]));
        }
        fn flush_devices(&self, interactive: bool) {
            self.record("FlushDevices", json!(interactive));
        }
        fn power_off(&self, interactive: bool) {
            self.record("PowerOff", json!(interactive));
        }
        fn reboot(&self, interactive: bool) {
            self.record("Reboot", json!(interactive));
        }
        fn suspend(&self, interactive: bool) {
            self.record("Suspend", json!(interactive));
        }
        fn hibernate(&self, interactive: bool) {
            self.record("Hibernate", json!(interactive));
        }
        fn sleep(&self, flags: u64) {
            self.record("Sleep", json!(flags));
        }
    }

    async fn fixture() -> (
        Connection,
        Connection,
        Calls,
        std::os::unix::net::UnixStream,
    ) {
        let calls = Calls::default();
        let (fifo, peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let fifo: std::os::fd::OwnedFd = fifo.into();
        let (a, b) = tokio::net::UnixStream::pair().unwrap();
        let server = Builder::unix_stream(a)
            .server(Guid::generate())
            .unwrap()
            .p2p()
            .serve_at(
                "/org/freedesktop/login1",
                Manager {
                    calls: calls.clone(),
                    fifo: fifo.into(),
                },
            )
            .unwrap()
            .build();
        let client = Builder::unix_stream(b).p2p().build();
        let (server, client) = tokio::try_join!(server, client).unwrap();
        (server, client, calls, peer)
    }

    #[tokio::test]
    async fn all_declared_methods_cross_the_real_dbus_wire() {
        let (_server, client, calls, _fifo_peer) = fixture().await;
        let proxy = Proxy::new(
            &client,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .await
        .unwrap();
        let leases = SessionLeases::default();
        let cases = [
            ("list_sessions", json!({})),
            ("list_users", json!({})),
            ("list_seats", json!({})),
            ("get_session", json!({"session_id":"s7"})),
            ("get_user", json!({"uid":1001})),
            ("get_seat", json!({"seat_id":"seat0"})),
            (
                "create_session",
                json!({"uid":1001,"pid":4321,"service":"fixture","type_":"tty","class":"user","desktop":"","seat_id":"seat0","vtnr":7,"tty":"tty7","display":"","remote":false,"remote_user":"","remote_host":"","properties":[{"name":"MemoryMax","value":{"type":"uint64","value":8192}}]}),
            ),
            ("release_session", json!({"session_id":"s7"})),
            ("terminate_session", json!({"session_id":"s7"})),
            ("lock_session", json!({"session_id":"s7"})),
            ("unlock_session", json!({"session_id":"s7"})),
            (
                "kill_session",
                json!({"session_id":"s7","who":"leader","signal_number":15}),
            ),
            ("terminate_user", json!({"uid":1001})),
            ("kill_user", json!({"uid":1001,"signal_number":15})),
            (
                "set_user_linger",
                json!({"uid":1001,"enable":true,"interactive":false}),
            ),
            ("terminate_seat", json!({"seat_id":"seat0"})),
            (
                "attach_device",
                json!({"seat_id":"seat0","sysfs_path":"/sys/devices/fixture","interactive":false}),
            ),
            ("flush_devices", json!({"interactive":false})),
            ("power_off", json!({"interactive":false})),
            ("reboot", json!({"interactive":false})),
            ("suspend", json!({"interactive":false})),
            ("hibernate", json!({"interactive":false})),
            ("sleep", json!({"flags":0})),
        ];
        let schema = login1_schema();
        assert_eq!(cases.len(), schema.methods.len());
        for (name, args) in cases {
            assert!(schema.methods.contains_key(name));
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                dispatch_with_proxy(&proxy, &leases, name, &args),
            )
            .await
            .expect("fixture timed out")
            .unwrap_or_else(|e| panic!("{name}: {e:#}"));
            match name {
                "list_sessions" => assert_eq!(result["sessions"][0]["uid"], 1001),
                "list_users" => assert_eq!(result["users"][0]["user"], "fixture"),
                "list_seats" => assert_eq!(result["seats"][0]["id"], "seat0"),
                "get_session" => assert_eq!(result["object_path"], "/session/s7"),
                "get_user" => assert_eq!(result["object_path"], "/user/u1001"),
                "get_seat" => assert_eq!(result["object_path"], "/seat/seat0"),
                "create_session" => {
                    assert_eq!(result["runtime_path"], "/runtime/1001");
                    assert!(leases.0.lock().await.contains_key("s7"));
                }
                _ => assert_eq!(result["disposition"], "accepted"),
            }
            if name == "release_session" {
                assert!(leases.0.lock().await.is_empty());
            }
        }
        {
            let recorded = calls.lock().unwrap();
            assert!(recorded.contains(&("KillSession".into(), json!(["s7", "leader", 15]))));
            assert!(recorded.contains(&(
                "AttachDevice".into(),
                json!(["seat0", "/sys/devices/fixture", false])
            )));
        }
        let error = dispatch_with_proxy(&proxy, &leases, "get_seat", &json!({"seat_id":"absent"}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("fixture missing seat"));
        assert!(dispatch_with_proxy(
            &proxy,
            &leases,
            "kill_session",
            &json!({"session_id":"s7","who":"anything","signal_number":9})
        )
        .await
        .is_err());
    }
}
