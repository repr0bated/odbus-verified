#!/bin/sh
# Idempotent install of Prometheus + node_exporter + Grafana OSS under runit on antix1.
# Usage: install.sh DL_DIR   (DL_DIR holds the sha256-verified, extracted upstream
# tarballs: prometheus-*/ node_exporter-*/ grafana-*/ and dash-{1860,3662,19727}.json)
set -eu
DL=$1
HERE=$(cd "$(dirname "$0")" && pwd)
P=$(ls -d "$DL"/prometheus-*.linux-amd64 | tail -1)
N=$(ls -d "$DL"/node_exporter-*.linux-amd64 | tail -1)
G=$(ls -d "$DL"/grafana-*/ | grep -v '\.tar' | tail -1); G=${G%/}

mkuser() { id -u "$1" >/dev/null 2>&1 || useradd --system --user-group --no-create-home --home-dir "$2" --shell /usr/sbin/nologin "$1"; }
mkuser prometheus /var/lib/prometheus
mkuser node_exporter /var/lib/node_exporter
mkuser grafana /var/lib/grafana

install -m 0755 "$P/prometheus" "$P/promtool" /usr/local/bin/
install -m 0755 "$N/node_exporter" /usr/local/bin/
gv=$(basename "$G")
if [ ! -d "/opt/$gv" ]; then cp -a "$G" "/opt/$gv.tmp" && mv "/opt/$gv.tmp" "/opt/$gv"; fi
chown -R root:root "/opt/$gv"
ln -sfn "/opt/$gv" /opt/grafana

install -d -m 0755 /etc/prometheus /etc/prometheus/rules
install -d -m 0750 -o root -g prometheus /etc/prometheus/tls
install -m 0644 "$HERE/prometheus/prometheus.yml" /etc/prometheus/prometheus.yml
install -m 0644 "$HERE"/prometheus/rules/*.yml /etc/prometheus/rules/
install -d -m 0750 -o prometheus -g prometheus /var/lib/prometheus
install -d -m 0755 -o node_exporter -g node_exporter /var/lib/node_exporter /var/lib/node_exporter/textfile

# Incus metrics-only client certificate (scoped to /1.0/metrics).
if command -v incus >/dev/null && [ ! -s /etc/prometheus/tls/incus-metrics.crt ]; then
  openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:secp384r1 -nodes -sha384 -days 3650 \
    -subj "/CN=prometheus@antix1" -keyout /etc/prometheus/tls/incus-metrics.key \
    -out /etc/prometheus/tls/incus-metrics.crt 2>/dev/null
  incus config trust add-certificate /etc/prometheus/tls/incus-metrics.crt --type=metrics --name=prometheus-antix1
fi
[ -f /var/lib/incus/server.crt ] && install -m 0644 /var/lib/incus/server.crt /etc/prometheus/tls/incus-server.crt
chown root:prometheus /etc/prometheus/tls/* ; chmod 0640 /etc/prometheus/tls/*
/usr/local/bin/promtool check config /etc/prometheus/prometheus.yml

install -d -m 0755 /etc/grafana /etc/grafana/provisioning /etc/grafana/provisioning/datasources /etc/grafana/provisioning/dashboards /etc/grafana/provisioning/plugins /etc/grafana/provisioning/alerting /etc/grafana/dashboards
install -m 0640 -o root -g grafana "$HERE/grafana/grafana.ini" /etc/grafana/grafana.ini
install -m 0644 "$HERE/grafana/provisioning/datasources/prometheus.yml" /etc/grafana/provisioning/datasources/
install -m 0644 "$HERE/grafana/provisioning/dashboards/files.yml" /etc/grafana/provisioning/dashboards/
install -m 0644 "$HERE"/grafana/dashboards/*.json /etc/grafana/dashboards/
install -d -m 0750 -o grafana -g grafana /var/lib/grafana /var/lib/grafana/plugins /var/lib/grafana/log

if [ ! -s /root/grafana-admin-password ]; then
  ( umask 077; head -c 32 /dev/urandom | base64 | tr -d '/+=\n' | cut -c1-32 > /root/grafana-admin-password )
fi
chown root:root /root/grafana-admin-password; chmod 0600 /root/grafana-admin-password

for s in node-exporter prometheus grafana; do
  install -d -m 0755 "/etc/sv/$s" "/etc/sv/$s/log"
  install -m 0755 "$HERE/sv/$s/run" "/etc/sv/$s/run"
  install -m 0755 "$HERE/sv/$s/log/run" "/etc/sv/$s/log/run"
  ln -sfn "/etc/sv/$s" "/etc/runit/runsvdir/default/$s"
done
echo "installed; runsvdir picks the services up within ~5s"
