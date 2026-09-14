#!/usr/bin/env bash
set -euo pipefail

if command -v wasm-pack >/dev/null 2>&1; then
  echo "wasm-pack already installed: $(wasm-pack --version)"
  exit 0
fi

install_via_cargo() {
  if ! command -v cargo >/dev/null 2>&1; then
    return 1
  fi
  echo "Installing wasm-pack with cargo..."
  cargo install wasm-pack --locked
}

is_musl_host() {
  if ! command -v ldd >/dev/null 2>&1; then
    return 1
  fi

  local out=""
  out="$(ldd --version 2>&1 || true)"
  if printf '%s' "${out}" | grep -qi "musl"; then
    return 0
  fi

  out="$(ldd 2>&1 || true)"
  if printf '%s' "${out}" | grep -qi "musl"; then
    return 0
  fi

  return 1
}

install_prebuilt() {
  local version="${WASM_PACK_VERSION:-0.14.0}"
  local arch
  arch="$(uname -m)"
  local os
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"

  if [[ "${os}" != "linux" || "${arch}" != "x86_64" ]]; then
    echo "Prebuilt fallback currently supports linux/x86_64 only." >&2
    return 1
  fi

  local out_dir="${HOME}/.local/bin"
  mkdir -p "${out_dir}"

  local base_url="https://github.com/rustwasm/wasm-pack/releases/download/v${version}"
  local candidates=(
    "wasm-pack-v${version}-x86_64-unknown-linux-musl.tar.gz"
    "wasm-pack-v${version}-x86_64-unknown-linux-gnu.tar.gz"
  )

  local tmp=""
  tmp="$(mktemp -d)"

  for archive in "${candidates[@]}"; do
    local url="${base_url}/${archive}"
    echo "Trying prebuilt wasm-pack: ${url}"
    if curl -fsSL "${url}" -o "${tmp}/${archive}"; then
      tar -xzf "${tmp}/${archive}" -C "${tmp}"
      local extracted
      extracted="$(find "${tmp}" -type f -name wasm-pack | head -n 1 || true)"
      if [[ -n "${extracted}" ]]; then
        install -m 0755 "${extracted}" "${out_dir}/wasm-pack"
        echo "Installed prebuilt wasm-pack to ${out_dir}/wasm-pack"
        rm -rf "${tmp}"
        return 0
      fi
    fi
  done

  rm -rf "${tmp}"
  return 1
}

# For musl hosts, prefer prebuilt binaries to avoid cargo linker/toolchain issues.
if is_musl_host; then
  echo "musl host detected; preferring prebuilt wasm-pack."
  if install_prebuilt; then
    echo "Installed: $(wasm-pack --version)"
    exit 0
  fi
  echo "Prebuilt install failed; attempting cargo install fallback..."
  if install_via_cargo; then
    echo "Installed: $(wasm-pack --version)"
    exit 0
  fi
else
  if install_via_cargo; then
    echo "Installed: $(wasm-pack --version)"
    exit 0
  fi

  echo "cargo install failed; attempting prebuilt wasm-pack download..."
  if install_prebuilt; then
    echo "Installed: $(wasm-pack --version)"
    exit 0
  fi
fi

echo "Failed to install wasm-pack via cargo and prebuilt fallback." >&2
exit 1
