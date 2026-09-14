//! Runit service-supervision paths — the single source of truth.
//!
//! This host boots **runit** as PID 1 and is controlled with `sv`. There is no
//! s6 installed: `s6-rc`, `s6-svc`, `s6-svstat` and `service6` are all absent,
//! so any code that spawns them fails at runtime. Import these constants
//! instead of hardcoding paths, so the layout is stated once.
//!
//! Verified layout:
//!
//! | Path | Meaning |
//! |---|---|
//! | `/etc/runit/sv/<svc>` | Service definition (`run`, optional `log/run`) |
//! | `/etc/runit/runsvdir/default/<svc>` | Symlink enabling the service at boot |
//! | `/run/runit/service` | Tree that `runsvdir -P` supervises |
//! | `/etc/runit/sv/<svc>/down` | Marker: do not auto-start when supervised |
//!
//! Runit has no compiled service database (unlike s6-rc): `runsvdir` picks up
//! directory changes on its own, so there is nothing to recompile after editing
//! a definition — edit the `run` script, then `sv restart <svc>`.
//!
//! OSCAL subid: `src.service.runit.paths@v1`

/// Service definitions live here, one directory per service.
pub const SV_DIR: &str = "/etc/runit/sv";

/// Enablement symlinks for the `default` runlevel (`current -> default`).
pub const RUNSVDIR_DEFAULT: &str = "/etc/runit/runsvdir/default";

/// The live supervised tree (`runsvdir -P /run/runit/service`).
pub const SERVICE_DIR: &str = "/run/runit/service";

/// The control binary. Requires root to read `supervise/ok`.
pub const SV_BIN: &str = "sv";

/// The supervisor process name, used for liveness probes.
pub const RUNSVDIR_PROC: &str = "runsvdir";

/// Definition directory for a service: `/etc/runit/sv/<service>`.
pub fn definition_path(service: &str) -> String {
    format!("{SV_DIR}/{service}")
}

/// Live supervised path for a service: `/run/runit/service/<service>`.
pub fn live_path(service: &str) -> String {
    format!("{SERVICE_DIR}/{service}")
}

/// Boot-persistent enablement symlink: `/etc/runit/runsvdir/default/<service>`.
pub fn enabled_path(service: &str) -> String {
    format!("{RUNSVDIR_DEFAULT}/{service}")
}

/// Supervise status file that `sv` and `runsv` maintain:
/// `/run/runit/service/<service>/supervise/stat`.
pub fn supervise_stat_path(service: &str) -> String {
    format!("{SERVICE_DIR}/{service}/supervise/stat")
}

/// Per-service log directory written by the `log/run` companion service.
pub fn log_current_path(service: &str) -> String {
    format!("/var/log/sv/{service}/current")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_match_the_verified_host_layout() {
        assert_eq!(definition_path("op-web"), "/etc/runit/sv/op-web");
        assert_eq!(live_path("op-web"), "/run/runit/service/op-web");
        assert_eq!(enabled_path("op-web"), "/etc/runit/runsvdir/default/op-web");
        assert_eq!(
            supervise_stat_path("op-web"),
            "/run/runit/service/op-web/supervise/stat"
        );
    }

    /// The s6 tree must never reappear in these constants.
    #[test]
    fn no_s6_paths() {
        for path in [SV_DIR, RUNSVDIR_DEFAULT, SERVICE_DIR] {
            assert!(!path.contains("s6"), "{path} still references s6");
        }
        assert_ne!(SERVICE_DIR, "/run/service", "that is the s6 tree");
    }
}
