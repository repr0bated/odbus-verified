#!/usr/bin/env bash
set -Eeuo pipefail

exporter=$(realpath "$(dirname "$0")/../export-verified-source.sh")
tmp=$(mktemp -d "${TMPDIR:-/tmp}/exporter-test.XXXXXX")
trap 'rm -rf -- "$tmp"' EXIT
src="$tmp/source with spaces"
mkdir -p "$src/crates/sample/src" "$src/crates/sample/.venv/bin" "$src/.agents/skills"
printf '[workspace]\nmembers=["crates/sample"]\nresolver="2"\n' > "$src/Cargo.toml"
printf '\n[package]\nname="root-example"\nversion="0.1.0"\nedition="2021"\n' >> "$src/Cargo.toml"
mkdir -p "$src/examples"
printf 'fn main() {}\n' > "$src/examples/check.rs"
printf 'private host material\n' > "$src/host-secrets.txt"
printf 'private env\n' > "$src/.env"
mkdir -p "$src/deploy"
printf 'private service environment\n' > "$src/deploy/service.env"
printf 'SERVICE_HOST=example.invalid\n' > "$src/deploy/service.env.example"
printf '[package]\nname="sample"\nversion="0.1.0"\nedition="2021"\n' > "$src/crates/sample/Cargo.toml"
printf 'fn main() {}\n' > "$src/crates/sample/src/main.rs"
(cd "$src" && cargo generate-lockfile --quiet)
printf 'instructions\n' > "$src/AGENTS.md"
cat > "$src/.gitignore" <<'EOF'
*.tar.gz
!vendor/tched-router/source.tar.gz
!vendor/3tched-json-render-ui/source.tar.gz
EOF
for skill in grpc-expert grpc-protocol-expert json-render ovs-db-analysis; do
    mkdir -p "$src/.agents/skills/$skill"
    printf 'skill\n' > "$src/.agents/skills/$skill/SKILL.md"
done
printf 'state source\n' > "$src/crates/sample/src/state.rs"
printf 'token source\n' > "$src/crates/sample/src/token.rs"
printf 'repomix source\n' > "$src/crates/sample/src/repomix.rs"
mkdir -p "$src/scripts/fixtures"
printf 'API_KEY=${API_KEY}\n' > "$src/scripts/fixtures/provider-config.sh"
mkdir -p "$src/vendor/tched-router" "$src/vendor/3tched-json-render-ui"
printf 'pinned native source artifact\n' > "$src/vendor/tched-router/source.tar.gz"
printf 'pinned UI source artifact\n' > "$src/vendor/3tched-json-render-ui/source.tar.gz"
printf 'API_KEY=sk-test-this-historical-file-must-not-export\n' > "$src/scripts/antigravity-full-setup.md"
ln -s state.rs "$src/crates/sample/src/internal-link.rs"
ln -s "$src/crates/sample/src/state.rs" "$src/crates/sample/src/absolute-link.rs"
ln -s /tmp "$src/crates/sample/.venv/bin/python"

success="$tmp/success"
"$exporter" "$src" "$success" >/dev/null
test -f "$success/crates/sample/src/state.rs"
test -f "$success/crates/sample/src/token.rs"
test -L "$success/crates/sample/src/internal-link.rs"
test "$(realpath "$success/crates/sample/src/absolute-link.rs")" = "$success/crates/sample/src/state.rs"
test -f "$success/crates/sample/src/repomix.rs"
test -f "$success/examples/check.rs"
test ! -e "$success/host-secrets.txt"
test ! -e "$success/.env"
test ! -e "$success/deploy/service.env"
test -f "$success/deploy/service.env.example"
test -f "$success/scripts/fixtures/provider-config.sh"
test ! -e "$success/scripts/antigravity-full-setup.md"
test -f "$success/.gitignore"
test -f "$success/vendor/tched-router/source.tar.gz"
test -f "$success/vendor/3tched-json-render-ui/source.tar.gz"
(
    cd "$success"
    git init -q
    git add .
    git ls-files --error-unmatch vendor/tched-router/source.tar.gz >/dev/null
    git ls-files --error-unmatch vendor/3tched-json-render-ui/source.tar.gz >/dev/null
)

unsafe_prefix=sk-or-
unsafe_suffix=this-active-literal-must-fail
printf 'API_KEY=%s%s\n' "$unsafe_prefix" "$unsafe_suffix" > "$src/scripts/fixtures/unsafe.sh"
unsafe="$tmp/unsafe"
if "$exporter" "$src" "$unsafe" >/dev/null 2>&1; then
    echo "credential-shaped active input was accepted" >&2
    exit 1
fi
test ! -e "$unsafe"
rm -f "$src/scripts/fixtures/unsafe.sh"

for key_kind in OPENSSH RSA EC DSA ENCRYPTED; do
    printf '%s%s%s\n' '-----BEGIN ' "$key_kind" ' PRIVATE KEY-----' > "$src/scripts/fixtures/unsafe.sh"
    if "$exporter" "$src" "$tmp/key-$key_kind" >/dev/null 2>&1; then
        echo "private key header was accepted" >&2
        exit 1
    fi
    test ! -e "$tmp/key-$key_kind"
done
rm -f "$src/scripts/fixtures/unsafe.sh"

mkdir -p "$tmp/rg-fails"
cat > "$tmp/rg-fails/rg" <<'EOF'
#!/usr/bin/env bash
exit 2
EOF
chmod +x "$tmp/rg-fails/rg"
rg_failed="$tmp/rg-failed"
if PATH="$tmp/rg-fails:$PATH" "$exporter" "$src" "$rg_failed" >/dev/null 2>&1; then
    echo "credential gate inspection error was accepted" >&2
    exit 1
fi
test ! -e "$rg_failed"
test ! -e "$success/crates/sample/.venv"

ln -s /tmp "$src/crates/sample/src/escaping-link"
failed="$tmp/failed"
if "$exporter" "$src" "$failed" >/dev/null 2>&1; then
    echo "escaping symlink was accepted" >&2
    exit 1
fi
test ! -e "$failed"

rm -f "$src/crates/sample/src/escaping-link" "$src/crates/sample/src/main.rs"
missing="$tmp/missing"
if "$exporter" "$src" "$missing" >/dev/null 2>&1; then
    echo "missing target was accepted" >&2
    exit 1
fi
test ! -e "$missing"
echo "exporter synthetic checks passed"
