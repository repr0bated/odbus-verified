#!/usr/bin/env bash
# Delegate the actual Cargo command; keep its arguments, output and status intact.
set -uo pipefail

dispatch_dir=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
dispatch_script="$dispatch_dir/cargo-dispatch.sh"
real_cargo=${CARGO_REAL_BINARY:-/usr/bin/cargo}
worker_effort=${CARGO_WORKER_REASONING_EFFORT:-low}
case "$worker_effort" in low|medium|high) ;; *) printf 'cargo dispatch: invalid worker reasoning effort\n' >&2; exit 125 ;; esac

fail() { printf 'cargo dispatch: %s\n' "$*" >&2; exit 125; }

if [[ ${1-} == --worker ]]; then
    [[ ${CARGO_DISPATCHED:-0} == 1 && $# == 2 ]] || fail 'worker entry requires a delegated request'
    request_dir=$2
    [[ -d $request_dir && -O $request_dir && ! -L $request_dir ]] || fail 'invalid worker request directory'
    [[ -f $request_dir/argv && -f $request_dir/cwd && -f $request_dir/real-cargo ]] || fail 'incomplete worker request'
    mapfile -d '' -t cargo_args < "$request_dir/argv"
    IFS= read -r -d '' cargo_cwd < "$request_dir/cwd" || [[ -n ${cargo_cwd:-} ]] || fail 'empty working directory'
    IFS= read -r -d '' real_cargo < "$request_dir/real-cargo" || [[ -n ${real_cargo:-} ]] || fail 'empty Cargo path'
    [[ $real_cargo == /* && -x $real_cargo ]] || fail 'native Cargo binary is unavailable'
    cd -- "$cargo_cwd" || fail 'working directory is unavailable'
    "$real_cargo" "${cargo_args[@]}" > "$request_dir/stdout" 2> "$request_dir/stderr"
    cargo_status=$?
    printf '%s\n' "$cargo_status" > "$request_dir/status.tmp"
    mv -- "$request_dir/status.tmp" "$request_dir/status"
    exit "$cargo_status"
fi

if [[ ${CARGO_DISPATCHED:-0} == 1 ]]; then
    [[ $real_cargo == /* && -x $real_cargo ]] || fail 'native Cargo binary is unavailable'
    exec "$real_cargo" "$@"
fi

# Explicit caller selection wins. Model names are hints, not authenticated identity.
family=${CARGO_CALLER_FAMILY:-}
if [[ -z $family ]]; then
    model_hint=${CARGO_CALLER_MODEL:-${CURSOR_MODEL:-${GROK_MODEL:-}}}
    case "${model_hint,,}" in
        *grok*) family=grok ;;
        *claude*|*haiku*|*sonnet*|*opus*|*fable*) family=claude ;;
        *gpt*|*codex*|*luna*) family=gpt ;;
        *)
            if [[ ${CLAUDECODE:-0} == 1 ]]; then family=claude
            elif [[ -n ${GROK_SESSION_ID:-} || -n ${GROK_MODEL:-} ]]; then family=grok
            else family=gpt
            fi ;;
    esac
fi
case "${family,,}" in
    gpt|codex|openai)
        family=gpt
        model=${CARGO_MODEL_GPT:-gpt-6-luna}
        runner=${CARGO_RUNNER_GPT:-codex}
        ;;
    grok|xai)
        family=grok
        model=${CARGO_MODEL_GROK:-composer-2.5}
        runner=${CARGO_RUNNER_GROK:-agent}
        ;;
    claude|anthropic)
        family=claude
        model=${CARGO_MODEL_CLAUDE:-haiku}
        runner=${CARGO_RUNNER_CLAUDE:-claude}
        ;;
    *) fail 'CARGO_CALLER_FAMILY must be gpt, grok or claude' ;;
esac

cargo_cwd=$PWD
git_root=$(git rev-parse --show-toplevel 2>/dev/null) || git_root=
if [[ -n $git_root && -f $git_root/Cargo.toml ]]; then cargo_cwd=$git_root; fi
if [[ ${CARGO_DISPATCH_DRY_RUN:-0} == 1 ]]; then
    printf 'caller=%s\nrunner=%s\nmodel=%s\ncwd=%s\n' "$family" "$runner" "$model" "$cargo_cwd"
    printf 'argv:'
    printf ' %q' "$real_cargo" "$@"
    printf '\n'
    exit 0
fi
command -v "$runner" >/dev/null 2>&1 || fail "$runner is unavailable; Cargo was not executed"
command -v flock >/dev/null 2>&1 || fail 'flock is unavailable; Cargo was not executed'
[[ $real_cargo == /* && -x $real_cargo ]] || fail 'native Cargo binary is unavailable'

umask 077
cache_dir=${CARGO_DISPATCH_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/opdbus-cargo-dispatch}
mkdir -p -- "$cache_dir" || fail 'cannot create private dispatch cache'
[[ -O $cache_dir && ! -L $cache_dir ]] || fail 'dispatch cache must belong to the caller'
workspace_key=$(printf '%s' "$cargo_cwd" | sha256sum)
workspace_key=${workspace_key%% *}
exec 9> "$cache_dir/$workspace_key.lock" || fail 'cannot open workspace dispatch lock'
flock -n 9 || fail 'a delegated Cargo command is already running in this workspace'
request_dir=$(mktemp -d "$cache_dir/request.XXXXXXXX") || fail 'cannot create private worker request'
cleanup_request() {
    if [[ -f $request_dir/status ]]; then
        rm -rf -- "$request_dir"
    else
        printf 'cargo dispatch: incomplete request retained privately at %s\n' "$request_dir" >&2
    fi
}
trap cleanup_request EXIT
printf '%s\0' "$@" > "$request_dir/argv"
# No arguments is different from one empty argument.
[[ $# != 0 ]] || : > "$request_dir/argv"
printf '%s\0' "$cargo_cwd" > "$request_dir/cwd"
printf '%s\0' "$real_cargo" > "$request_dir/real-cargo"
printf -v worker_command '%q --worker %q' "$dispatch_script" "$request_dir"
prompt="You are the owner's delegated Cargo execution worker ($model). Read this repository's AGENTS.md, HANDOFF.md and four mandatory SKILL.md files completely before execution. The owner explicitly assigns this one Cargo command to you; execute it yourself without another delegation. Do not edit source, grants, services, credentials, storage configuration or HANDOFF. Do not add Cargo flags or run additional Cargo commands. Run exactly this fixed request helper, which preserves the owner's arguments and captures native Cargo output/status:
$worker_command
Wait until the helper finishes, including polling a running tool session. Report completion or the exact failure; do not treat a running process as completed. The calling dispatcher owns shared task/handoff updates."
prompt+="
Do not send a final response while the helper's session is still running. A running-session final response terminates this worker and loses the build. Continue polling that exact session until it returns an exit code and the helper has written its completion receipt. Use at most 60 seconds per poll; do not launch the helper twice."
prompt+="
Use the native Bash tool for that literal command. Copy the absolute helper path verbatim: it is NOT /usr/bin/cargo-dispatch.sh. Your working directory is already correct and CARGO_DISPATCHED=1 is inherited. Do not prepend cd or reconstruct the command. Do not use MCP process tools."

printf 'cargo: %s → %s\n' "$family" "$model" >&2
export CARGO_DISPATCHED=1 CARGO_CALLER_FAMILY=$family
case "$family" in
    gpt)
        writable_args=(--add-dir "$request_dir")
        for build_cache in "${CARGO_HOME:-$HOME/.cargo}" "${RUSTUP_HOME:-$HOME/.rustup}"; do
            [[ ! -d $build_cache ]] || writable_args+=(--add-dir "$build_cache")
        done
        "$runner" exec --model "$model" -c "model_reasoning_effort=\"$worker_effort\"" \
            -c 'sandbox_workspace_write.network_access=true' --sandbox workspace-write \
            "${writable_args[@]}" --output-last-message "$request_dir/summary" -C "$cargo_cwd" \
            "$prompt" </dev/null > "$request_dir/runner.log" 2>&1
        ;;
    grok)
        "$runner" --print --model "$model" --workspace "$cargo_cwd" \
            "$prompt" </dev/null > "$request_dir/runner.log" 2>&1
        ;;
    claude)
        (cd -- "$cargo_cwd" && "$runner" --print --model "$model" --permission-mode dontAsk \
            --add-dir "$request_dir" --strict-mcp-config --mcp-config '{"mcpServers":{}}' \
            --tools Read,Bash \
            --allowedTools "Read" "Bash($dispatch_script *)" "Bash(cd $cargo_cwd)" -- "$prompt") \
            </dev/null > "$request_dir/runner.log" 2>&1
        ;;
esac
runner_status=$?
if [[ ! -f $request_dir/status ]]; then
    printf 'cargo dispatch: %s did not complete the requested command (runner status %s).\n' "$model" "$runner_status" >&2
    if [[ -s $request_dir/summary ]]; then cat -- "$request_dir/summary" >&2
    else tail -n 18 -- "$request_dir/runner.log" >&2
    fi
    exit 125
fi
IFS= read -r cargo_status < "$request_dir/status"
[[ $cargo_status =~ ^[0-9]+$ && $cargo_status -le 255 ]] || fail 'invalid Cargo completion status'
cat -- "$request_dir/stdout"
cat -- "$request_dir/stderr" >&2
exit "$cargo_status"
