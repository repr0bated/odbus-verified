#!/bin/bash
# Installation script for op-xray-daemon D-Bus service
# Sets up D-Bus policy and service files

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

echo "Installing op-xray-daemon..."

# Check if running as root (required for system bus operations)
if [ "$EUID" -ne 0 ]; then
    echo "Warning: Not running as root. May need sudo for some operations."
fi

# This host uses runit; refuse to install a service definition for another
# supervisor.
if ! command -v sv >/dev/null 2>&1; then
    echo "Error: runit sv command not found"
    exit 1
fi

echo "Detected runit: $(command -v sv)"

# Create D-Bus policy file
DBUS_POLICY_DIR="/etc/dbus-1/system.d"
if [ ! -d "$DBUS_POLICY_DIR" ]; then
    echo "Creating D-Bus policy directory..."
    mkdir -p "$DBUS_POLICY_DIR"
fi

cat > "${DBUS_POLICY_DIR}/org.opdbus.v1.Xray.conf" << 'EOF'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <!-- Policy for org.opdbus.v1.Xray service -->

  <!-- Allow root to own the service -->
  <policy user="root">
    <allow own="org.opdbus.v1.plugins"/>
    <allow send_destination="org.opdbus.v1.plugins"/>
    <allow receive_sender="org.opdbus.v1.plugins"/>
  </policy>

  <!-- Allow anyone to call the Xray interface methods -->
  <policy context="default">
    <allow send_destination="org.opdbus.v1.plugins"/>
    <allow send_destination="org.opdbus.v1.plugins"
           send_interface="org.opdbus.v1.Xray"/>
    <allow receive_sender="org.opdbus.v1.plugins"/>
  </policy>

  <!-- Allow specific group members full access -->
  <policy group="opdbus">
    <allow own="org.opdbus.v1.plugins"/>
    <allow send_destination="org.opdbus.v1.plugins"/>
    <allow send_interface="org.opdbus.v1.Xray"/>
    <allow receive_sender="org.opdbus.v1.plugins"/>
  </policy>
</busconfig>
EOF

echo "Created D-Bus policy: ${DBUS_POLICY_DIR}/org.opdbus.v1.Xray.conf"

# Install the binary
BINARY_PATH="${PROJECT_ROOT}/target/release/op-xray-daemon"
if [ ! -f "$BINARY_PATH" ]; then
    echo "Release binary not found, building..."
    cd "$PROJECT_ROOT"
    cargo build --release -p op-xray-daemon
fi

# Copy binary to system location
INSTALL_DIR="/usr/local/bin"
if [ -d /usr/local/sbin ]; then
    INSTALL_DIR="/usr/local/sbin"
fi

install -m 755 "$BINARY_PATH" "${INSTALL_DIR}/op-xray-daemon"
echo "Installed binary to: ${INSTALL_DIR}/op-xray-daemon"

RUNIT_SERVICE_DIR="/etc/runit/sv/op-xray-daemon"
RUNIT_RUNSVDIR="/etc/runit/runsvdir/default"
mkdir -p "$RUNIT_SERVICE_DIR"
cat > "${RUNIT_SERVICE_DIR}/run" << EOF
#!/bin/sh
exec 2>&1
exec ${INSTALL_DIR}/op-xray-daemon
EOF
chmod +x "${RUNIT_SERVICE_DIR}/run"
if [ ! -e "${RUNIT_RUNSVDIR}/op-xray-daemon" ]; then
    ln -s "$RUNIT_SERVICE_DIR" "${RUNIT_RUNSVDIR}/op-xray-daemon"
fi
echo "Created runit service: ${RUNIT_SERVICE_DIR}"

# Reload D-Bus configuration
echo "Reloading D-Bus configuration..."
if [ -x /bin/dbus-send ] || [ -x /usr/bin/dbus-send ]; then
    dbus-send --system --type=method_call --dest=org.freedesktop.DBus \
        /org/freedesktop/DBus org.freedesktop.DBus.ReloadConfig 2>/dev/null || true
fi

echo ""
echo "Installation complete!"
echo ""
echo "Start the daemon with:"
echo "  sv up op-xray-daemon"
echo ""
echo "Test with:"
echo "  dbus-send --system --dest=org.opdbus.v1.plugins --type=method_call /org/opdbus/v1/plugins/xray org.opdbus.v1.Xray.Status"
echo ""
