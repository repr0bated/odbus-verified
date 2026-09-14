# op-xray-daemon

D-Bus service for managing the Xray proxy daemon lifecycle.

## Overview

This daemon provides a D-Bus interface (`org.opdbus.v1.Xray`) for controlling the xray proxy process. It replaces direct `Command::new("xray")` subprocess spawning with a schema-driven D-Bus service, adhering to the AGENTS.md D-Bus-first architecture rules.

## D-Bus Interface

**Bus Name:** `org.opdbus.v1.plugins`

**Object Path:** `/org/opdbus/v1/plugins/xray`

**Interface:** `org.opdbus.v1.Xray`

### Methods

| Method | Arguments | Returns | Description |
|--------|-----------|---------|-------------|
| `Start` | `config_path: String` | `(bool, String)` | Start xray with the specified config file |
| `Stop` | - | `(bool, String)` | Stop the running xray process |
| `Restart` | `config_path: String` | `(bool, String)` | Stop then start xray with new config |
| `Status` | - | `String` | JSON object with running status, pid, config_path |
| `Reload` | - | `(bool, String)` | Send SIGHUP to xray to reload config |
| `GetConfig` | - | `String` | Return current config path or empty string |

### Status JSON Format

```json
{
  "running": true,
  "pid": 12345,
  "config_path": "/etc/xray/xray_config.json",
  "uptime_secs": 3600,
  "start_time": "2024-06-05T10:00:00"
}
```

## Configuration Path

Xray's live configuration must exist only at this path inside the container:

```
/etc/xray/xray_config.json
```

The static bootstrap is materialized there during boot. A future validated
control-plane generator replaces that same file atomically and asks the bridge
to reload Xray; models do not write or reload Xray directly.

## Usage

### Starting the Daemon

```bash
# Run with default config path
op-xray-daemon

# Run with explicit config path
op-xray-daemon --config /etc/xray/xray_config.json
```

### Using D-Bus Methods

#### Start xray

```bash
dbus-send --system --dest=org.opdbus.v1.plugins --type=method_call \
  /org/opdbus/v1/plugins/xray org.opdbus.v1.Xray.Start \
  string:"/etc/xray/xray_config.json"
```

#### Check Status

```bash
dbus-send --system --dest=org.opdbus.v1.plugins --type=method_call \
  /org/opdbus/v1/plugins/xray org.opdbus.v1.Xray.Status
```

#### Reload Config

```bash
dbus-send --system --dest=org.opdbus.v1.plugins --type=method_call \
  /org/opdbus/v1/plugins/xray org.opdbus.v1.Xray.Reload
```

#### Stop xray

```bash
dbus-send --system --dest=org.opdbus.v1.plugins --type=method_call \
  /org/opdbus/v1/plugins/xray org.opdbus.v1.Xray.Stop
```

## Replacing Direct Subprocess Spawning

Previously, code would spawn xray directly:

```rust
// OLD: Direct subprocess spawn (FORBIDDEN per AGENTS.md)
Command::new("xray")
    .arg("-c")
    .arg("/etc/xray/xray_config.json")
    .spawn()
```

Now, use the D-Bus service instead:

```rust
// NEW: D-Bus method call (REQUIRED per AGENTS.md)
use zbus::Connection;

let conn = Connection::system().await?;
let proxy = conn.call_method(
    "org.opdbus.v1.plugins",
    "/org/opdbus/v1/plugins/xray",
    "org.opdbus.v1.Xray",
    "Start",
    &("/etc/xray/xray_config.json",)
).await?;
```

## Installation

See `deploy/install-op-xray-daemon.sh` for D-Bus policy and service installation.

## Architecture

- **D-Bus First:** All xray control operations go through D-Bus methods
- **Process Lifecycle:** The daemon maintains a handle to the xray child process
- **Signals:** Graceful shutdown on SIGTERM/SIGINT, properly terminating xray
- **Logging:** Structured logging via `tracing`
- **Config Location:** `/etc/xray/xray_config.json` inside the Xray container

## Dependencies

- `xray` binary in PATH
- D-Bus system bus
- tokio runtime

## License

MIT OR Apache-2.0
