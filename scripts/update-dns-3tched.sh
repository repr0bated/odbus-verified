#!/bin/bash
# Update 3tched.com mail-adjacent DNS records on Cloudflare without stomping
# provider-managed mail routing.

set -euo pipefail

DOMAIN="3tched.com"

die() {
    printf 'ERROR: %s\n' "$*" >&2
    exit 1
}

load_credentials() {
    [ -f ~/.bash_secrets ] || die "~/.bash_secrets not found"
    # shellcheck disable=SC1090
    source ~/.bash_secrets

    CF_TOKEN="${CF_DNS_ZONE_TOKEN:-}"
    ZONE_ID="${CF_ZONEID_3TCHEDCOM:-${CF_ZONE_ID_3TCHED:-${CF_ZONEID_3TCHED:-}}}"

    [ -n "$CF_TOKEN" ] || die "CF_DNS_ZONE_TOKEN missing from ~/.bash_secrets"
    [ -n "$ZONE_ID" ] || die "CF_ZONEID_3TCHEDCOM missing from ~/.bash_secrets"
}

detect_public_ipv4() {
    ip -4 route get 1.1.1.1 2>/dev/null |
        awk '/src/ {for (i = 1; i <= NF; i++) if ($i == "src") {print $(i + 1); exit}}'
}

cf_api() {
    local method="$1"
    local endpoint="$2"
    local payload="${3:-}"
    local response

    if [ -n "$payload" ]; then
        response=$(
            curl -fsS -X "$method" "https://api.cloudflare.com/client/v4${endpoint}" \
                -H "Authorization: Bearer ${CF_TOKEN}" \
                -H "Content-Type: application/json" \
                --data "$payload"
        )
    else
        response=$(
            curl -fsS -X "$method" "https://api.cloudflare.com/client/v4${endpoint}" \
                -H "Authorization: Bearer ${CF_TOKEN}" \
                -H "Content-Type: application/json"
        )
    fi

    echo "$response" | jq -e '.success == true' >/dev/null ||
        die "Cloudflare API request failed for ${method} ${endpoint}: $response"

    printf '%s\n' "$response"
}

record_fqdn() {
    local name="$1"
    if [ "$name" = "@" ]; then
        printf '%s\n' "$DOMAIN"
    else
        printf '%s.%s\n' "$name" "$DOMAIN"
    fi
}

list_records() {
    local type="$1"
    local name="$2"
    local fqdn
    fqdn="$(record_fqdn "$name")"
    cf_api GET "/zones/${ZONE_ID}/dns_records?type=${type}&name=${fqdn}"
}

delete_record() {
    local record_id="$1"
    [ -n "$record_id" ] || return 0
    cf_api DELETE "/zones/${ZONE_ID}/dns_records/${record_id}" >/dev/null
}

delete_matching_ids() {
    while IFS= read -r record_id; do
        [ -n "$record_id" ] || continue
        delete_record "$record_id"
    done
}

create_record() {
    local payload="$1"
    cf_api POST "/zones/${ZONE_ID}/dns_records" "$payload" >/dev/null
}

upsert_a_record() {
    local name="$1"
    local content="$2"
    local ttl="$3"
    local proxied="$4"
    local records

    records="$(list_records A "$name")"
    echo "$records" | jq -r '.result[].id' | delete_matching_ids

    create_record "$(jq -nc \
        --arg type "A" \
        --arg name "$name" \
        --arg content "$content" \
        --argjson ttl "$ttl" \
        --argjson proxied "$proxied" \
        '{type:$type,name:$name,content:$content,ttl:$ttl,proxied:$proxied}')"
}

upsert_mx_record() {
    local name="$1"
    local content="$2"
    local priority="$3"
    local ttl="$4"
    local records

    records="$(list_records MX "$name")"
    echo "$records" | jq -r '.result[].id' | delete_matching_ids

    create_record "$(jq -nc \
        --arg type "MX" \
        --arg name "$name" \
        --arg content "$content" \
        --argjson priority "$priority" \
        --argjson ttl "$ttl" \
        '{type:$type,name:$name,content:$content,priority:$priority,ttl:$ttl}')"
}

upsert_txt_with_prefix() {
    local name="$1"
    local prefix="$2"
    local content="$3"
    local ttl="$4"
    local records

    records="$(list_records TXT "$name")"
    echo "$records" |
        jq -r --arg prefix "$prefix" '.result[] | select(.content | startswith($prefix)) | .id' |
        delete_matching_ids

    create_record "$(jq -nc \
        --arg type "TXT" \
        --arg name "$name" \
        --arg content "$content" \
        --argjson ttl "$ttl" \
        '{type:$type,name:$name,content:$content,ttl:$ttl}')"
}

replace_all_txt_records() {
    local name="$1"
    local content="$2"
    local ttl="$3"
    local records

    records="$(list_records TXT "$name")"
    echo "$records" | jq -r '.result[].id' | delete_matching_ids

    create_record "$(jq -nc \
        --arg type "TXT" \
        --arg name "$name" \
        --arg content "$content" \
        --argjson ttl "$ttl" \
        '{type:$type,name:$name,content:$content,ttl:$ttl}')"
}

detect_public_mx() {
    command -v dig >/dev/null 2>&1 || return 1
    dig +short MX "$DOMAIN" 2>/dev/null | awk 'NF >= 2 {print $2; exit}' | sed 's/\.$//'
}

load_credentials

MAIL_IP="${1:-$(detect_public_ipv4)}"
[ -n "$MAIL_IP" ] || die "could not detect public IPv4 automatically; pass it as the first argument"

SPF_VALUE="v=spf1 ip4:${MAIL_IP} ~all"
DMARC_VALUE="v=DMARC1; p=quarantine; rua=mailto:admin@${DOMAIN}; ruf=mailto:admin@${DOMAIN}; fo=1"
PUBLIC_MX="$(detect_public_mx || true)"

printf 'Updating DNS records for %s\n' "$DOMAIN"
printf 'Current public IPv4: %s\n' "$MAIL_IP"

if [[ "$PUBLIC_MX" =~ ^_dc-mx\..*\.${DOMAIN}$ ]]; then
    printf 'Detected Cloudflare-managed public MX: %s\n' "$PUBLIC_MX"
    printf 'Preserving provider-managed MX instead of forcing mail.%s\n' "$DOMAIN"
else
    printf 'No provider-managed MX detected; setting direct MX to mail.%s\n' "$DOMAIN"
fi

printf '\nUpserting webmail hostname...\n'
upsert_a_record "mail" "$MAIL_IP" 1 true

printf 'Upserting SPF record...\n'
upsert_txt_with_prefix "@" "v=spf1 " "$SPF_VALUE" 300

printf 'Canonicalizing DMARC record...\n'
replace_all_txt_records "_dmarc" "$DMARC_VALUE" 300

if [[ "$PUBLIC_MX" =~ ^_dc-mx\..*\.${DOMAIN}$ ]]; then
    printf 'Skipping MX mutation to avoid overriding Cloudflare-managed mail routing.\n'
else
    printf 'Upserting apex MX record...\n'
    upsert_mx_record "@" "mail.${DOMAIN}" 10 300
fi

printf 'Leaving DKIM selectors unchanged; preserve existing live selectors unless rotating keys.\n'

printf '\nCurrent public DNS:\n'
if command -v dig >/dev/null 2>&1; then
    printf '  MX:   %s\n' "$(dig +short MX "$DOMAIN" | paste -sd ';' -)"
    printf '  A:    %s\n' "$(dig +short A "mail.${DOMAIN}" | paste -sd ';' -)"
    printf '  SPF:  %s\n' "$(dig +short TXT "$DOMAIN" | grep 'v=spf1' || true)"
    printf '  DMARC:%s\n' " $(dig +short TXT "_dmarc.${DOMAIN}" | paste -sd ';' -)"
fi
