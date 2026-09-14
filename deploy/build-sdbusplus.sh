#!/bin/bash
# Build sdbusplus locally on Artix/s6 — elogind drop-in for systemd-libs
# Run from any directory; outputs to ./sdbusplus-build/

set -euo pipefail

REPO_URL="https://github.com/openbmc/sdbusplus.git"
BUILD_DIR="${BUILD_DIR:-$(pwd)/sdbusplus-build}"
SRC_DIR="${BUILD_DIR}/src"
PREFIX="${PREFIX:-/usr/local}"
JOBS="${JOBS:-$(nproc)}"

echo "=== sdbusplus Artix/s6 local build ==="
echo "Build dir : ${BUILD_DIR}"
echo "Install prefix: ${PREFIX}"

# 1. Ensure elogind is installed (provides sd-bus/sd-event/sd-daemon headers + libelogind)
if ! pkg-config --exists libelogind; then
    echo "ERROR: libelogind not found. Install elogind first:"
    echo "  sudo pacman -S elogind"
    exit 1
fi

# 2. Clone or update source
mkdir -p "${BUILD_DIR}"
if [ -d "${SRC_DIR}/.git" ]; then
    echo "Updating existing clone..."
    git -C "${SRC_DIR}" pull --ff-only
else
    echo "Cloning ${REPO_URL}..."
    git clone --depth 1 "${REPO_URL}" "${SRC_DIR}"
fi

# 3. Patch meson.build to use libelogind instead of libsystemd
#    elogind on Artix provides the full sd-bus/sd-event/sd-daemon headers
#    under /usr/include/elogind/systemd/ and exports all sd_bus_* symbols.
sed -i "s/dependency('libsystemd')/dependency('libelogind')/g" \
    "${SRC_DIR}/meson.build"

# 4. Meson configure
echo "Configuring with meson..."
rm -rf "${BUILD_DIR}/build"
meson setup "${BUILD_DIR}/build" "${SRC_DIR}" \
    --prefix="${PREFIX}" \
    --buildtype=release \
    -Dtests=disabled \
    -Dexamples=disabled

# 5. Compile
meson compile -C "${BUILD_DIR}/build" -j"${JOBS}"

# 6. Install (requires sudo for system paths)
echo "Installing to ${PREFIX} ..."
sudo meson install -C "${BUILD_DIR}/build"

# 7. Python tools
echo "Installing Python tools..."
cd "${SRC_DIR}/tools"
python -m build --wheel --no-isolation
sudo python -m installer --destdir="/" dist/*.whl

echo ""
echo "=== Done ==="
echo "Installed:"
echo "  Library : ${PREFIX}/lib/libsdbusplus.so"
echo "  Headers : ${PREFIX}/include/sdbusplus/"
echo "  Pkg-config: ${PREFIX}/lib/pkgconfig/sdbusplus.pc"
echo ""
echo "Verify with: pkg-config --libs sdbusplus"
