#!/bin/sh
# Print the value for this request context.
#
#   lungs
#       identity_sled schema hash compared on the lungs read
#   sealed_id_digest
#       blake3 hex of the sealed-id bytes on stdin
#   grants <principal-id> <capability>
#       granted or denied from capability-grants.json
set -eu

catalog_dir="${OP_BLOB_CATALOG_DIR:-/dev/shm/opdbus/plugin-blobs}"
grants_path="${OP_GRANTS_PATH:-/dev/shm/opdbus/capability-grants.json}"

usage() {
    echo "usage: request-context-value.sh lungs" >&2
    echo "       request-context-value.sh sealed_id_digest   # sealed id on stdin" >&2
    echo "       request-context-value.sh grants <principal-id> <capability>" >&2
    exit 2
}

[ $# -ge 1 ] || usage
context=$1
shift

case "$context" in
lungs)
    [ $# -eq 0 ] || usage
    [ -r "$catalog_dir/.manifest.json" ] || {
        echo "sealed catalog manifest unavailable" >&2
        exit 1
    }
    jq -er '.plugins.identity_sled | strings | select(test("^[0-9a-fA-F]{64}$"))' \
        "$catalog_dir/.manifest.json"
    ;;
sealed_id_digest)
    [ $# -eq 0 ] || usage
    b3sum --no-names
    ;;
grants | capability-grants.json)
    [ $# -eq 2 ] || usage
    principal=$1
    capability=$2
    [ -r "$grants_path" ] || {
        echo "grant projection unavailable" >&2
        exit 1
    }
    jq -er --arg principal "$principal" --arg capability "$capability" '
        def groups: ["$ROOT","$ADMIN","$SUPERADMIN","$EXTERNAL-MCP","$VENDOR","$GUEST","$AGENT-L1","$AGENT-L2","$AGENT-L3","$AGENT-L4"];
        def badkey:
            . as $key
            | $key == "*"
              or ($key | test("^[0-9a-fA-F]{64}$"))
              or (($key | startswith("$")) and (groups | index($key) | not));
        def blank: test("^[[:space:]]*$");
        def entry_ok($doc; $key):
            type == "object"
            and (.capabilities | type == "array")
            and all(.capabilities[]; type == "string" and (blank | not))
            and ((.capabilities | length) == (.capabilities | unique | length))
            and ((has("display_alias") | not) or (.display_alias | type == "string"))
            and (
                if ($key | startswith("$")) then has("groups") | not
                elif ($key | length) == 0 then false
                elif has("groups") then
                    (.groups | type == "array")
                    and all(.groups[]; type == "string")
                    and ((.groups | length) == (.groups | unique | length))
                    and all(.groups[]; . as $group | (groups | index($group)) != null and ($doc | has($group)))
                else true end
            );
        if type != "object" then "denied"
        elif any(keys[]; badkey) then "denied"
        elif ($principal == "" or $principal == "*" or ($principal | startswith("$"))) then "denied"
        else . as $doc
            | if any($doc | keys[] as $key | $doc[$key] | entry_ok($doc; $key) | not) then "denied"
            elif ($doc | has($principal) | not) then "denied"
            else
                [ $doc[$principal].capabilities[],
                  ($doc[$principal].groups // [])[] as $group | $doc[$group].capabilities[] ]
                | if index($capability) then "granted" else "denied" end
            end
        end
    ' "$grants_path"
    ;;
*)
    usage
    ;;
esac
