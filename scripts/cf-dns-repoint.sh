#!/usr/bin/env bash
# Fix every Cloudflare A record that still targets the old decoy VM: repoint
# ghostbridge.tech names at the current decoy, delete the retired 3tched.com
# console names, leave the 3tched.com site (Cloudflare static page) alone.
#
#   CF_API_TOKEN=... deploy/scripts/cf-dns-repoint.sh            # dry run
#   CF_API_TOKEN=... deploy/scripts/cf-dns-repoint.sh --apply
#
# The token needs Zone:DNS:Edit on both zones. It is read from CF_API_TOKEN or
# from /etc/op-dbus/secrets/cf-dns-token (root-owned, 0600). Only records whose
# content equals OLD_IP are touched; proxied flag and TTL are preserved.
set -euo pipefail

OLD_IP="${OLD_IP:-129.153.134.63}"
NEW_IP="${NEW_IP:-129.80.64.127}"
ZONES="${ZONES:-3tched.com ghostbridge.tech}"
# Names never repointed at a decoy: the 3tched.com site itself lives on Cloudflare
# (static page); its apex/www stay proxied Cloudflare records whatever they hold.
KEEP="${KEEP:-3tched.com www.3tched.com}"
# Names deleted outright: they served the retired operator console (pairing
# login) and have no replacement — the chat UI is reached through the fabric.
DELETE="${DELETE:-dashboard.3tched.com api.3tched.com registration.3tched.com}"
API="https://api.cloudflare.com/client/v4"
apply=0
[[ "${1:-}" == "--apply" ]] && apply=1

token="${CF_API_TOKEN:-}"
if [[ -z "$token" && -r /etc/op-dbus/secrets/cf-dns-token ]]; then
  token="$(tr -d '[:space:]' < /etc/op-dbus/secrets/cf-dns-token)"
fi
[[ -n "$token" ]] || { echo "no Cloudflare token: set CF_API_TOKEN or install /etc/op-dbus/secrets/cf-dns-token" >&2; exit 2; }
[[ "$token" != *$'\n'* && "$token" != *$'\r'* ]] || { echo 'Cloudflare token must be one line' >&2; exit 2; }

cf() {
  printf 'Authorization: Bearer %s\n' "$token" |
    curl -sS --fail-with-body -H @- -H "Content-Type: application/json" "$@"
}

for zone in $ZONES; do
  zone_id="$(cf "$API/zones?name=$zone" | jq -r '.result[0].id // empty')"
  [[ -n "$zone_id" ]] || { echo "$zone: zone not visible to this token" >&2; exit 3; }
  cf "$API/zones/$zone_id/dns_records?type=A&per_page=200" \
    | jq -c --arg old "$OLD_IP" '.result[] | select(.content == $old) | {id, name, content, proxied, ttl}' \
    | while read -r rec; do
        id="$(jq -r .id <<<"$rec")"; name="$(jq -r .name <<<"$rec")"
        case " $KEEP " in *" $name "*) echo "keep          $name  (Cloudflare static page)"; continue;; esac
        case " $DELETE " in *" $name "*)
          if (( apply )); then
            cf -X DELETE "$API/zones/$zone_id/dns_records/$id" | jq -r --arg n "$name" '"deleted  " + $n'
          else
            echo "would delete  $name  ($OLD_IP; retired console)"
          fi
          continue;;
        esac
        proxied="$(jq -r .proxied <<<"$rec")"; ttl="$(jq -r .ttl <<<"$rec")"
        if (( apply )); then
          cf -X PATCH "$API/zones/$zone_id/dns_records/$id" \
            --data "$(jq -nc --arg ip "$NEW_IP" --argjson p "$proxied" --argjson t "$ttl" '{content:$ip, proxied:$p, ttl:$t}')" \
            | jq -r --arg n "$name" '"updated  " + $n + " -> " + .result.content'
        else
          echo "would update  $name  $OLD_IP -> $NEW_IP  (proxied=$proxied ttl=$ttl)"
        fi
      done
done
(( apply )) || echo "dry run only; re-run with --apply"
