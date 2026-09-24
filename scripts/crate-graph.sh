#!/usr/bin/env bash
# Build the OP-DBUS crate graph and emit it as CozoScript for op-cozo-store.
#
# Nodes (graph_node.label)      Edges (graph_edge.rel)
#   crate                         depends_on | dev_depends_on | build_depends_on  crate -> crate
#   ext_crate                     uses_ext                                        crate -> ext_crate
#   module (one per .rs file)     contains                                        crate -> module
#   bin                           imports                                         module -> crate (op_x:: refs)
#   plugin                        provides_bin / bin_entry                        crate -> bin / bin -> module
#   proto_service                 registers_plugin / defines_plugin               crate -> plugin / module -> plugin
#   runit_service                 declares_proto_service                          crate -> proto_service
#                                 runs_bin                                        runit_service -> bin
#
# Usage: scripts/crate-graph.sh [out-dir]   (default docs/crate-graph)
# scripts/hooks/pre-commit runs this when a commit stages crates, manifests,
# or deploy/runit. Install it with:
#   ln -sfn ../../scripts/hooks/pre-commit .git/hooks/pre-commit
# Load:  cargo run -p op-cozo-store --example load_crate_graph -- docs/crate-graph/crate-graph.cozo
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
OUT="${1:-docs/crate-graph}"
mkdir -p "$OUT"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT

cargo metadata --format-version 1 --no-deps > "$TMP/meta.json"

# ---------- nodes / edges as TSV: id \t label \t props-json ----------
NODES="$TMP/nodes.tsv"; EDGES="$TMP/edges.tsv"; : > "$NODES"; : > "$EDGES"
node() { printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$NODES"; }
edge() { printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "${4:-{\}}" >> "$EDGES"; }

# crates -------------------------------------------------------------------
jq -r '.packages[] | [.name, (.manifest_path | rtrimstr("/Cargo.toml")), (.description // ""),
        ([.targets[] | select(.kind[]=="bin") | .name] | join(",")),
        ([.targets[] | select(.kind[]=="lib" or .kind[]=="rlib") | .name] | length)] | map(tostring) | join("\u001f")' "$TMP/meta.json" \
| while IFS=$'\x1f' read -r name path desc bins haslib; do
    rel="${path#"$PWD"/}"; [ "$rel" = "$PWD" ] && rel="."
    files=0; loc=0; tests=0; unsafe=0; todo=0; unwrap=0
    if [ -d "$rel/src" ] || [ "$rel" = "." ]; then
      srcdir="$rel"; [ "$rel" = "." ] && srcdir="src"
      if [ -d "$srcdir" ]; then
        files=$(find "$srcdir" -name '*.rs' -not -path '*/target/*' | wc -l || true)
        loc=$(find "$srcdir" -name '*.rs' -not -path '*/target/*' -exec cat {} + 2>/dev/null | wc -l || true)
        tests=$(grep -rE '^\s*#\[(tokio::)?test' "$srcdir" --include=*.rs 2>/dev/null | wc -l || true)
        unsafe=$(grep -rEw 'unsafe' "$srcdir" --include=*.rs 2>/dev/null | wc -l || true)
        todo=$(grep -rE 'TODO|FIXME|XXX|HACK' "$srcdir" --include=*.rs 2>/dev/null | wc -l || true)
        unwrap=$( (grep -rE '\.unwrap\(\)|\.expect\(' "$srcdir" --include=*.rs 2>/dev/null || true) | grep -vc "test" || true); unwrap=${unwrap:-0}
      fi
    fi
    node "crate:$name" crate "$(jq -cn --arg p "$rel" --arg d "$desc" --arg b "$bins" \
        --argjson files "$files" --argjson loc "$loc" --argjson tests "$tests" \
        --argjson unsafe "$unsafe" --argjson todo "$todo" --argjson unwrap "$unwrap" --argjson lib "$haslib" \
        '{path:$p, description:$d, bins:($b|split(",")|map(select(.!=""))), has_lib:($lib>0),
          rs_files:$files, loc:$loc, tests:$tests, unsafe_refs:$unsafe, todo:$todo, unwrap_expect:$unwrap}')"
    # binaries
    for b in ${bins//,/ }; do
      src=$(jq -r --arg n "$name" --arg b "$b" '.packages[] | select(.name==$n) | .targets[] | select(.kind[]=="bin" and .name==$b) | .src_path' "$TMP/meta.json")
      src="${src#"$PWD"/}"
      node "bin:$b" bin "$(jq -cn --arg c "$name" --arg s "$src" '{crate:$c, src:$s}')"
      edge "crate:$name" provides_bin "bin:$b"
      edge "bin:$b" bin_entry "mod:$src"
    done
  done

# dependencies ---------------------------------------------------------------
jq -r '.packages[] as $p | $p.dependencies[] | [$p.name, (.kind // "normal"), .name, (.path != null), .optional, (.req // "")] | map(tostring) | join("\u001f")' "$TMP/meta.json" \
| while IFS=$'\x1f' read -r from kind to internal optional req; do
    case "$kind" in normal) rel=depends_on;; dev) rel=dev_depends_on;; build) rel=build_depends_on;; *) rel=depends_on;; esac
    if [ "$internal" = "true" ]; then
      edge "crate:$from" "$rel" "crate:$to" "$(jq -cn --argjson o "$optional" '{optional:$o}')"
    else
      node "ext:$to" ext_crate "$(jq -cn --arg r "$req" '{req:$r}')"
      edge "crate:$from" uses_ext "ext:$to" "$(jq -cn --arg k "$kind" --argjson o "$optional" '{kind:$k, optional:$o}')"
    fi
  done

# modules + imports ------------------------------------------------------------
# crate name <-> rust ident map
jq -r '.packages[] | "\(.name)\t\(.name | gsub("-";"_"))"' "$TMP/meta.json" > "$TMP/crate-idents.tsv"
jq -r '.packages[] | "\(.name)\t\(.manifest_path | rtrimstr("/Cargo.toml"))"' "$TMP/meta.json" \
| while IFS=$'\t' read -r name path; do
    rel="${path#"$PWD"/}"
    [ "$rel" = "$PWD" ] && continue          # root package has no src
    find "$rel" -name '*.rs' -not -path '*/target/*' | sort | while read -r f; do
      kind=src
      case "$f" in */tests/*) kind=test;; */examples/*) kind=example;; */benches/*) kind=bench;; */src/bin/*) kind=bin;; */build.rs) kind=build;; esac
      loc=$(wc -l < "$f")
      node "mod:$f" module "$(jq -cn --arg c "$name" --arg k "$kind" --argjson l "$loc" '{crate:$c, kind:$k, loc:$l}')"
      edge "crate:$name" contains "mod:$f"
      # cross-crate references: op_xxx:: tokens
      (grep -vE '^\s*//' "$f" | grep -oE '\bop_[a-z0-9_]+::' 2>/dev/null || true) | sort | uniq -c | while read -r cnt tok; do
        ident="${tok%::}"
        target=$(awk -F'\t' -v i="$ident" '$2==i{print $1}' "$TMP/crate-idents.tsv")
        if [ -n "$target" ] && [ "$target" != "$name" ]; then
          edge "mod:$f" imports "crate:$target" "$(jq -cn --argjson n "$cnt" '{refs:$n}')"
        fi
      done
    done
  done

# plugins (inventory::submit! { PluginReg::new("<id>" | PLUGIN_NAME, ...) }) ---
grep -rlE 'PluginReg::new\(' crates/op-plugins/src --include=*.rs | sort | while read -r f; do
  # collapse whitespace so multi-line calls match; keep line of the call for props
  tr '\n' ' ' < "$f" | grep -oE 'PluginReg::new\(\s*("[a-z0-9_]+"|[A-Z_]+)' | sed -E 's/PluginReg::new\(\s*//' | sort -u | while read -r arg; do
    case "$arg" in
      \"*) id="${arg//\"/}";;
      *)   id=$( (grep -oE "const $arg: &str = \"[a-z0-9_]+\"" "$f" || true) | head -1 | sed -E 's/.*"([a-z0-9_]+)"/\1/');;
    esac
    [ -n "$id" ] || { echo "warn: unresolved plugin id $arg in $f" >&2; continue; }
    line=$( (grep -nE 'PluginReg::new\(' "$f" || true) | head -1 | cut -d: -f1)
    # a plugin only exists if its module is declared (inventory::submit! in an unreachable file never compiles)
    modname=$(basename "$f" .rs); moddir=$(dirname "$f"); declared=false
    if [ "$modname" = "mod" ]; then declared=true
    elif grep -qE "^\s*(pub(\([a-z]+\))?\s+)?mod\s+$modname\s*;" "$moddir/mod.rs" "$moddir.rs" 2>/dev/null; then declared=true; fi
    node "plugin:$id" plugin "$(jq -cn --arg f "$f" --argjson l "${line:-0}" --arg id "$id" --argjson d "$declared" '{file:$f, line:$l, declared:$d, dbus_path:("/org/opdbus/v1/plugins/"+$id)}')"
    edge "crate:op-plugins" registers_plugin "plugin:$id"
    edge "mod:$f" defines_plugin "plugin:$id"
  done
done

# proto services --------------------------------------------------------------
for p in crates/op-grpc-bridge/proto/*.proto; do
  pkg=$( (grep -E '^package ' "$p" || true) | head -1 | sed -E 's/package ([^;]+);/\1/')
  (grep -E '^service ' "$p" || true) | sed -E 's/service ([A-Za-z0-9_]+).*/\1/' | while read -r svc; do
    node "proto:$pkg.$svc" proto_service "$(jq -cn --arg f "$p" --arg pkg "$pkg" '{file:$f, package:$pkg}')"
    edge "crate:op-grpc-bridge" declares_proto_service "proto:$pkg.$svc"
  done
done

# runit services -> workspace binaries -----------------------------------------
enabled=$(grep -vE '^\s*(#|$)' deploy/runit/enabled-services 2>/dev/null || true)
retired=$(grep -vE '^\s*(#|$)' deploy/runit/retired-services 2>/dev/null || true)
bins=$(jq -r '.packages[].targets[] | select(.kind[]=="bin") | .name' "$TMP/meta.json" | sort -u)
for run in deploy/runit/*/run; do
  svc=$(basename "$(dirname "$run")")
  en=false; rt=false
  if grep -qx "$svc" <<<"$enabled"; then en=true; fi
  if grep -qx "$svc" <<<"$retired"; then rt=true; fi
  for b in $bins; do
    if grep -vE 'wait-ready|runit-ready-signal|^\s*#' "$run" | grep -E "(exec|chpst|/usr/local/bin/|/opt/op-dbus/|target/release/)[^#]*(^|[[:space:]/\"'])$b([[:space:]\"']|$)" >/dev/null 2>&1; then
      node "runit:$svc" runit_service "$(jq -cn --arg r "$run" --argjson e "$en" --argjson t "$rt" '{run:$r, enabled:$e, retired:$t}')"
      edge "runit:$svc" runs_bin "bin:$b"
    fi
  done
done

# provenance node (CozoScript multi-block scripts cannot start with a comment) --
node "meta:crate-graph" meta "$(jq -cn --arg t "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg c "$(git rev-parse HEAD)" \
  '{generated_at:$t, commit:$c, generator:"scripts/crate-graph.sh", relations:["graph_node","graph_edge"]}')"

# ---------- emit -------------------------------------------------------------
sort -u "$NODES" > "$TMP/nodes.u"; sort -u "$EDGES" > "$TMP/edges.u"
emit_block() { # $1=tsv $2=ncols(3|4) $3=put-clause
  local tsv="$1" cols="$2" put="$3"
  awk -F'\t' -v cols="$cols" -v put="$put" '
    function q(s){ if (index(s, "\"___")) { print "unencodable literal: " s > "/dev/stderr"; exit 1 } return "___\"" s "\"___" }
    NR%400==1 { if (NR>1) printf "]\n%s\n}\n", put; printf "{\n?[%s] <- [\n", (cols==3?"id, label, props":"src, rel, dst, props") }
    { line = "[" q($1) ", " q($2) ", " q($3); if (cols==4) line = line ", " q($4); line = line "]"; printf "%s%s\n", (NR%400==1?"":","), line }
    END { if (NR>0) printf "]\n%s\n}\n", put }' "$tsv"
}
{
  emit_block "$TMP/nodes.u" 3 ":put graph_node { id => label, props }"
  emit_block "$TMP/edges.u" 4 ":put graph_edge { src, rel, dst => props }"
} > "$OUT/crate-graph.cozo"
jq -Rn --slurpfile _ /dev/null '
  { nodes: [inputs | select(startswith("N\t")) | split("\t") | {id:.[1], label:.[2], props:(.[3]|fromjson)}] }' \
  <(sed 's/^/N\t/' "$TMP/nodes.u") > "$TMP/n.json"
jq -Rn '{ edges: [inputs | split("\t") | {src:.[0], rel:.[1], dst:.[2], props:(.[3]|fromjson)}] }' "$TMP/edges.u" > "$TMP/e.json"
jq -s '.[0] + .[1]' "$TMP/n.json" "$TMP/e.json" > "$OUT/crate-graph.json"
echo "wrote $OUT/crate-graph.cozo ($(wc -l < "$TMP/nodes.u") nodes, $(wc -l < "$TMP/edges.u") edges) and $OUT/crate-graph.json"
