#!/usr/bin/env bash
# One-time (and repeatable) server set-up, run as root on the server:
#
#   sudo bash install.sh weather.example.co.za
#
# Adds the weather app next to whatever the server already runs behind Caddy
# (as on the kwaai server): a `weather` system user, a systemd service on
# 127.0.0.1, a Caddy site in /etc/caddy/sites/, and the newest release.
# Running it again keeps the existing access token and data.

set -euo pipefail

DOMAIN="${1:-}"
PORT="${WEATHER_PORT:-8787}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

die() { echo "install: $*" >&2; exit 1; }

[[ -n "$DOMAIN" ]] || die "usage: sudo bash install.sh weather.example.co.za"
[[ $EUID -eq 0 ]] || die "run with sudo"
command -v caddy >/dev/null || die "Caddy isn't installed. This adds a site to an existing Caddy (see docs/DEPLOYMENT.md)."
[[ -f /etc/caddy/Caddyfile ]] || die "/etc/caddy/Caddyfile not found"
command -v curl >/dev/null || die "curl is missing (apt-get install -y curl)"
command -v openssl >/dev/null || die "openssl is missing (apt-get install -y openssl)"

# Something else on our port would make the health check pass for the wrong app.
if ss -ltnH "sport = :$PORT" | grep -q . && ! systemctl is-active --quiet weather; then
   die "port $PORT is already in use; pick another with WEATHER_PORT=8788 sudo -E bash install.sh $DOMAIN"
fi

echo "==> User and folders"
id weather >/dev/null 2>&1 || useradd --system --home-dir /var/lib/weather --shell /usr/sbin/nologin weather
install -d -o weather -g weather -m 0700 /var/lib/weather
install -d -m 0755 /opt/weather /opt/weather/releases
install -d -o root -g weather -m 0750 /etc/weather

echo "==> Configuration (/etc/weather/weather.env)"
ENV=/etc/weather/weather.env
NEW_TOKEN=""
if [[ ! -f "$ENV" ]]; then
   NEW_TOKEN="$(openssl rand -hex 24)"
   cat > "$ENV" <<EOF
# Read by weather.service. Keep this file private: OWNER_TOKEN unlocks custom
# sources and MCP. Change it with: sudo weather-token
OWNER_TOKEN=$NEW_TOKEN
ALLOWED_HOSTS=$DOMAIN
PORT=$PORT
RUST_LOG=weather_server=info,tower_http=warn
EOF
else
   # Keep the token; follow a changed domain or port.
   sed -i "s/^ALLOWED_HOSTS=.*/ALLOWED_HOSTS=$DOMAIN/; s/^PORT=.*/PORT=$PORT/" "$ENV"
fi
chown root:weather "$ENV"
chmod 0640 "$ENV"

echo "==> Scripts"
install -m 0755 "$HERE/update.sh" /usr/local/bin/weather-update
cat > /usr/local/bin/weather-token <<'EOF'
#!/usr/bin/env bash
# Replaces the owner token and restarts the server. Devices and MCP clients
# then need the new one.
set -euo pipefail
[[ $EUID -eq 0 ]] || { echo "run with sudo" >&2; exit 1; }
token="$(openssl rand -hex 24)"
sed -i "s/^OWNER_TOKEN=.*/OWNER_TOKEN=$token/" /etc/weather/weather.env
systemctl restart weather
echo "New access token: $token"
EOF
chmod 0755 /usr/local/bin/weather-token

echo "==> Service (weather.service)"
cat > /etc/systemd/system/weather.service <<EOF
[Unit]
Description=Weather dashboard and MCP server
After=network-online.target
Wants=network-online.target

[Service]
User=weather
Group=weather
EnvironmentFile=/etc/weather/weather.env
Environment=HOST=127.0.0.1
Environment=DATA_DIR=/var/lib/weather
Environment=WEB_DIST=/opt/weather/current/web
WorkingDirectory=/var/lib/weather
ExecStart=/opt/weather/current/weather-server
Restart=on-failure
RestartSec=2

# It only needs its data folder and outbound HTTPS.
NoNewPrivileges=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/weather
ProtectHome=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
RestrictNamespaces=yes
LockPersonality=yes
MemoryDenyWriteExecute=yes
CapabilityBoundingSet=
SystemCallArchitectures=native
MemoryMax=300M

[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload
systemctl enable weather >/dev/null

echo "==> Caddy site (/etc/caddy/sites/weather.caddy)"
install -d -m 0755 /etc/caddy/sites
cat > /etc/caddy/sites/weather.caddy <<EOF
# Written by the weather app's deploy/install.sh; running it again overwrites it
$DOMAIN {
   encode zstd gzip

   # The app shell and service worker have no version in their names:
   # browsers must check them each time or they keep an old app.
   @shell path / /index.html /sw.js /manifest.webmanifest /registerSW.js
   header @shell Cache-Control "no-cache"

   # MCP event streams are flushed as they come (Caddy does this for SSE).
   reverse_proxy 127.0.0.1:$PORT
}
EOF

IMPORT_LINE="import /etc/caddy/sites/*.caddy"
if ! grep -qxF "$IMPORT_LINE" /etc/caddy/Caddyfile; then
   printf '\n# Other apps on this server (e.g. the weather app)\n%s\n' "$IMPORT_LINE" >> /etc/caddy/Caddyfile
   echo "   Added '$IMPORT_LINE' to /etc/caddy/Caddyfile."
   echo "   If another app's setup script rewrites that file, it must keep this line."
fi
caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null

echo "==> Installing the newest release"
/usr/local/bin/weather-update

systemctl reload caddy

echo "==> Checking https://$DOMAIN (the first certificate can take a minute)"
ok=""
for _ in $(seq 1 30); do
   if curl -fsS -m 5 "https://$DOMAIN/api/health" >/dev/null 2>&1; then ok=1; break; fi
   sleep 2
done
if [[ -n "$ok" ]]; then
   echo "   https://$DOMAIN is up."
else
   echo "!! https://$DOMAIN isn't answering yet. Check the DNS record and: sudo journalctl -u caddy -n 50 --no-pager" >&2
fi

if [[ -n "$NEW_TOKEN" ]]; then
   cat <<EOF

Access token (shown once; it is also in $ENV):

   $NEW_TOKEN

Enter it in the app under Settings → Server access, and give it to MCP clients:

   claude mcp add --transport http weather https://$DOMAIN/mcp --header "Authorization: Bearer $NEW_TOKEN"
EOF
fi
