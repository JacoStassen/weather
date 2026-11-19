#!/usr/bin/env bash
# Run on your Mac from the project folder. Copies the deploy scripts to the
# server and runs the set-up there. The app itself comes from the newest
# GitHub release (built by CI on every push to master), not from this Mac.
#
#   ./deploy/push.sh ubuntu@92.4.129.38 weather.stassen.org.za
#
# Safe to run again (keeps the access token and data). For updates alone:
#   ssh ubuntu@92.4.129.38 sudo weather-update

set -euo pipefail

HOST="${1:-}"
DOMAIN="${2:-}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -z "$HOST" || -z "$DOMAIN" ]]; then
   echo "Usage: ./deploy/push.sh ubuntu@YOUR_SERVER_IP weather.yourdomain.co.za"
   exit 1
fi

echo "==> Copying deploy scripts to ${HOST}"
ssh "$HOST" 'mkdir -p weather-deploy'
scp -q "$HERE/install.sh" "$HERE/update.sh" "$HOST:weather-deploy/"

echo "==> Running set-up on the server"
ssh -t "$HOST" "sudo bash weather-deploy/install.sh '$DOMAIN'"
