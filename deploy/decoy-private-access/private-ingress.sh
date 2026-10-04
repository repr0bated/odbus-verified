#!/bin/bash
# Owns only the decoy's private-ingress nftables table. It denies new or
# established TCP traffic to SSH/REALITY from every interface outside the
# private allowlist, without changing any other firewall state.
set -euo pipefail
umask 077
export LC_ALL=C

readonly TABLE=op_decoy_private_ingress
readonly CHAIN=input
readonly TABLE_COMMENT=op-decoy-private-ingress-owned
readonly RULE_COMMENT=op-decoy-private-ingress-deny
readonly RECEIPT_NAME=receipt.json

die() { printf 'private-ingress: %s\n' "$*" >&2; exit 1; }
for tool in nft jq flock stat install mktemp; do
    command -v "$tool" >/dev/null 2>&1 || die "$tool is required"
done
[[ $EUID -eq 0 ]] || die 'must run with root privileges'

action=${1:-check}
case $action in install|check|remove) ;; *) die 'usage: private-ingress.sh {install|check|remove}' ;; esac
state=${OP_DECOY_PRIVATE_INGRESS_STATE_DIR:-/run/op-decoy-private-ingress}
[[ $state == /* && $state != *'/../'* && $state != */.. ]] || die 'state path must be absolute and contain no parent traversal'
if [[ -e $state || -L $state ]]; then
    [[ -d $state && ! -L $state ]] || die 'state path is not a real directory'
    [[ $(stat -c '%u:%a' -- "$state") == "0:700" ]] || die 'state directory must be root-owned mode 0700'
else
    install -d -o 0 -g 0 -m 0700 -- "$state"
fi

exec 8>"$state/operation.lock"
flock -n 8 || die 'another private-ingress operation is running'

receipt=$state/$RECEIPT_NAME
tmp_receipt=
rules_file=
cleanup() {
    [[ -z ${tmp_receipt:-} || ! -e $tmp_receipt ]] || rm -- "$tmp_receipt"
    [[ -z ${rules_file:-} || ! -e $rules_file ]] || rm -- "$rules_file"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' HUP TERM

receipt_valid() {
    [[ -f $receipt && ! -L $receipt ]] || return 1
    jq -e '
      . == {
        format:"op-decoy-private-ingress/v1",
        family:"inet",
        table:"op_decoy_private_ingress",
        chain:"input",
        priority:-20,
        interfaces:["lo","tun5","wg-opdbus","wg0"],
        tcp_ports:[22,9443]
      }
    ' "$receipt" >/dev/null 2>&1
}

table_exists() {
    local document
    document=$(nft -j list tables 2>/dev/null) || die 'cannot read nftables table inventory'
    jq -e 'has("nftables") and (.nftables | type == "array")' \
      <<<"$document" >/dev/null 2>&1 || die 'nftables table inventory is not valid JSON'
    jq -e --arg table "$TABLE" \
      'any(.nftables[].table?; .family == "inet" and .name == $table)' \
      <<<"$document" >/dev/null 2>&1
}

table_exact() {
    local document
    document=$(nft -j list table inet "$TABLE" 2>/dev/null) || return 1
    jq -e --arg table "$TABLE" --arg table_comment "$TABLE_COMMENT" \
      --arg chain "$CHAIN" --arg rule_comment "$RULE_COMMENT" '
      [.nftables[] | .table? // empty] as $tables |
      [.nftables[] | .chain? // empty] as $chains |
      [.nftables[] | .rule? // empty] as $rules |
      ([.nftables[] | keys[]] | all(. == "metainfo" or . == "table" or . == "chain" or . == "rule")) and
      ($tables | length) == 1 and
      $tables[0].family == "inet" and $tables[0].name == $table and
      $tables[0].comment == $table_comment and
      ($chains | length) == 1 and
      $chains[0].family == "inet" and $chains[0].table == $table and
      $chains[0].name == $chain and $chains[0].type == "filter" and
      $chains[0].hook == "input" and $chains[0].prio == -20 and
      $chains[0].policy == "accept" and
      ($rules | length) == 1 and
      $rules[0].family == "inet" and $rules[0].table == $table and
      $rules[0].chain == $chain and $rules[0].comment == $rule_comment and
      ($rules[0].expr | length) == 3 and
      $rules[0].expr[0].match.op == "!=" and
      $rules[0].expr[0].match.left == {meta:{key:"iifname"}} and
      ($rules[0].expr[0].match.right.set | sort) == ["lo","tun5","wg-opdbus","wg0"] and
      $rules[0].expr[1].match.op == "==" and
      $rules[0].expr[1].match.left == {payload:{protocol:"tcp",field:"dport"}} and
      ($rules[0].expr[1].match.right.set | sort) == [22,9443] and
      $rules[0].expr[2] == {drop:null}
    ' <<<"$document" >/dev/null 2>&1
}

write_receipt() {
    tmp_receipt=$(mktemp "$state/.receipt.XXXXXXXX")
    chmod 0600 "$tmp_receipt"
    cat >"$tmp_receipt" <<'JSON'
{"format":"op-decoy-private-ingress/v1","family":"inet","table":"op_decoy_private_ingress","chain":"input","priority":-20,"interfaces":["lo","tun5","wg-opdbus","wg0"],"tcp_ports":[22,9443]}
JSON
    mv -- "$tmp_receipt" "$receipt"
    tmp_receipt=
}

write_rules() {
    rules_file=$(mktemp "$state/.rules.XXXXXXXX")
    chmod 0600 "$rules_file"
    cat >"$rules_file" <<'NFT'
table inet op_decoy_private_ingress {
  comment "op-decoy-private-ingress-owned"
  chain input {
    type filter hook input priority -20; policy accept;
    iifname != { "lo", "tun5", "wg-opdbus", "wg0" } tcp dport { 22, 9443 } drop comment "op-decoy-private-ingress-deny"
  }
}
NFT
}

case $action in
    install)
        if [[ -e $receipt || -L $receipt ]]; then
            receipt_valid || die 'ownership receipt differs; refusing to change firewall state'
        elif table_exists; then
            die 'table exists without an ownership receipt; refusing to overwrite it'
        fi
        if table_exists; then
            table_exact || die 'existing owned table differs; refusing to overwrite it'
            printf 'private-ingress: already installed and exact\n'
            exit 0
        fi
        write_rules
        [[ -e $receipt ]] || write_receipt
        nft --check --file "$rules_file"
        nft --file "$rules_file"
        table_exact || die 'post-install normalized readback differs; table retained for fail-closed inspection'
        printf 'private-ingress: installed and exact\n'
        ;;
    check)
        receipt_valid || die 'ownership receipt is absent or differs'
        table_exact || die 'owned table is absent or differs'
        printf 'private-ingress: present and exact\n'
        ;;
    remove)
        if ! table_exists; then
            if [[ -e $receipt || -L $receipt ]]; then
                receipt_valid || die 'table is absent but ownership receipt differs; refusing cleanup'
                rm -- "$receipt"
            fi
            printf 'private-ingress: already absent\n'
            exit 0
        fi
        receipt_valid || die 'ownership receipt is absent or differs; refusing to delete table'
        table_exact || die 'owned table differs; refusing to delete it'
        nft delete table inet "$TABLE"
        if table_exists; then die 'table remains after delete request'; fi
        rm -- "$receipt"
        printf 'private-ingress: removed owned table\n'
        ;;
esac
