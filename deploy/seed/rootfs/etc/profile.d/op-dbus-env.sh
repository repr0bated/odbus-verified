# op-dbus variable store. Settings: /etc/op-dbus/env; secrets (group
# opdbus-secrets): /etc/op-dbus/secrets. File name is the variable name in
# lowercase kebab-case: openai-api-key -> OPENAI_API_KEY. Files with other
# names (e.g. cloudflare-dns.ini) are read by path, not exported.
for _op_f in /etc/op-dbus/env/* /etc/op-dbus/secrets/*; do
    [ -f "$_op_f" ] && [ -r "$_op_f" ] || continue
    _op_n=${_op_f##*/}
    case $_op_n in *[!a-z0-9-]*|-*|[0-9]*) continue ;; esac
    _op_n=$(printf %s "$_op_n" | tr 'a-z-' 'A-Z_')
    export "$_op_n=$(cat "$_op_f")"
done
unset _op_f _op_n
