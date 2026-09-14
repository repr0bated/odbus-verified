# op-s6-systemctl

D-Bus service that maps `systemctl` commands to `s6`/`s6-rc` operations for Artix Linux systems using s6 as the init system.

## Overview

This daemon provides a D-Bus interface (`org.opdbus.v1.S6.Systemctl`) that translates traditional systemd commands to their s6 equivalents, enabling compatibility with tools and scripts that expect systemctl-like behavior.

## D-Bus Interface

**Bus Name**: `org.opdbus.v1.S6.Systemctl`
**Object Path**: `/org/opdbus/v1/plugins/s6/systemctl`
**Interface**: `org.opdbus.v1.S6.Systemctl`

## Method Mapping

| Method | systemctl Equivalent | s6 Equivalent | Description |
|--------|---------------------|---------------|-------------|
| `start(service)` | `start <svc>` | `s6-rc -u change <svc>` | Start a service |
| `stop(service)` | `stop <svc>` | `s6-rc -d change <svc>` | Stop a service |
| `restart(service)` | `restart <svc>` | `stop` then `start` | Restart a service |
| `reload(service)` | `reload <svc>` | `s6-svc -h <svc>` | Send SIGHUP for config reload |
| `enable(service)` | `enable <svc>` | `s6-rc-bundle add default <svc>` | Enable service at boot |
| `disable(service)` | `disable <svc>` | `s6-rc-bundle delete default <svc>` | Disable service at boot |
| `status(service)` | `status <svc>` | `s6-svstat <svc>` | Get detailed service status (JSON) |
| `is_active(service)` | `is-active <svc>` | `s6-svstat <svc>` | Returns "active" or "inactive" |
| `is_enabled(service)` | `is-enabled <svc>` | Check `/etc/s6-rc/default/` | Returns "enabled" or "disabled" |
| `list_units()` | `list-units` | `s6-rc -a list` | List all active units (JSON) |
| `daemon_status()` | N/A | `pgrep s6-svscan` | Returns "running" or "not-available" |

## Method Signatures

```rust
// Start/stop/restart/reload/enable/disable all return (success: bool, message: String)
async fn start(service: &str) -> (bool, String);
async fn stop(service: &str) -> (bool, String);
async fn restart(service: &str) -> (bool, String);
async fn reload(service: &str) -> (bool, String);
async fn enable(service: &str) -> (bool, String);
async fn disable(service: &str) -> (bool, String);

// Status methods return strings
async fn status(service: &str) -> String;        // JSON with ActiveState, SubState, MainPID
async fn is_active(service: &str) -> String;     // "active" or "inactive"
async fn is_enabled(service: &str) -> String;    // "enabled" or "disabled"
async fn list_units() -> String;                // JSON array of service objects
async fn daemon_status() -> String;             // "running" or "not-available"
```

## Status JSON Format

The `status()` method returns JSON in this format:

```json
{
  "name": "nginx",
  "active_state": "active",
  "sub_state": "running",
  "main_pid": 1234,
  "ready": true,
  "up_time": "3600"
}
```

The `list_units()` method returns a JSON array of status objects.

## Error Handling

All methods provide helpful error messages when s6 tools are not available or when operations fail. The service gracefully handles:

- Missing s6 binaries
- Invalid service names
- Permission errors
- Service not found errors

## Building

```bash
cargo build -p op-s6-systemctl
cargo build -p op-s6-systemctl --release
```

## Running

```bash
# Run directly (requires D-Bus system bus access)
sudo ./target/release/op-s6-systemctl

# Or with cargo
sudo cargo run -p op-s6-systemctl
```

## Dependencies

- Artix Linux with s6 init system
- `s6`, `s6-rc`, `s6-svc`, `s6-svstat` tools
- D-Bus system bus

## Architecture

Following the 3tched Architecture D-Bus-first principles:

- All service operations go through D-Bus methods
- No direct subprocess spawning from clients
- Schema-driven interface with consistent error handling
- Zero-copy shared memory design patterns

## License

Apache-2.0
