#!/usr/bin/env bash
# Installs a release from GitHub and restarts the server. Rolls back if the
# new build doesn't come up healthy. install.sh copies this script to
# /usr/local/bin/weather-update.
#
#   sudo weather-update                  # newest release
#   sudo weather-update build-12         # a specific one (also to roll back)
#
# GitHub Actions runs it over SSH with a key that may run nothing else; the
# key's forced command passes the requested tag as the argument, which is
# checked below (see docs/DEPLOYMENT.md).

set -euo pipefail

REPO="${WEATHER_REPO:-JacoStassen/weather}"
ASSET="weather-linux-arm64.tar.gz"
ROOT=/opt/weather
KEEP=3

die() { echo "weather-update: $*" >&2; exit 1; }

[[ $EUID -eq 0 ]] || die "run with sudo"
[[ "$(uname -m)" == "aarch64" ]] || die "releases are built for Arm64 (Oracle Ampere); this machine is $(uname -m)"

TAG="${1:-}"
TAG="${TAG// /}"
if [[ -n "$TAG" && ! "$TAG" =~ ^build-[0-9]+$ ]]; then
   die "not a release tag: '$TAG' (expected build-<number>)"
fi

if [[ -n "$TAG" ]]; then
   URL="https://github.com/${REPO}/releases/download/${TAG}"
else
   URL="https://github.com/${REPO}/releases/latest/download"
fi

PORT="$(sed -n 's/^PORT=//p' /etc/weather/weather.env 2>/dev/null | tail -1)"
PORT="${PORT:-8787}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> Downloading ${TAG:-the latest release}"
curl -fsSL --retry 3 -o "$TMP/$ASSET" "$URL/$ASSET"
curl -fsSL --retry 3 -o "$TMP/$ASSET.sha256" "$URL/$ASSET.sha256"
(cd "$TMP" && sha256sum --quiet -c "$ASSET.sha256") || die "checksum mismatch, not installing"

mkdir -p "$TMP/x"
tar -xzf "$TMP/$ASSET" -C "$TMP/x"
read -r BUILD COMMIT < "$TMP/x/BUILD"
[[ "$BUILD" =~ ^build-[0-9]+$ ]] || die "release has no valid BUILD file"

DEST="$ROOT/releases/$BUILD"
CURRENT="$(readlink -f "$ROOT/current" 2>/dev/null || true)"

if [[ "$CURRENT" == "$DEST" ]] && systemctl is-active --quiet weather; then
   echo "==> $BUILD ($COMMIT) is already running"
   exit 0
fi

rm -rf "$DEST"
mkdir -p "$ROOT/releases"
mv "$TMP/x" "$DEST"
chown -R root:root "$DEST"
chmod -R a+rX,go-w "$DEST"

switch_to() {
   ln -sfn "$1" "$ROOT/current.new"
   mv -T "$ROOT/current.new" "$ROOT/current"
   systemctl restart weather
}

healthy() {
   for _ in $(seq 1 20); do
      if curl -fsS -m 2 -H 'Host: localhost' "http://127.0.0.1:${PORT}/api/health" >/dev/null 2>&1; then
         return 0
      fi
      sleep 0.5
   done
   return 1
}

echo "==> Starting $BUILD ($COMMIT)"
switch_to "$DEST"

if ! healthy; then
   echo "!! $BUILD did not become healthy. Last log lines:" >&2
   journalctl -u weather -n 20 --no-pager >&2 || true
   if [[ -n "$CURRENT" && -d "$CURRENT" ]]; then
      echo "==> Rolling back to $(basename "$CURRENT")" >&2
      switch_to "$CURRENT"
      healthy || echo "!! the previous build is not healthy either" >&2
   fi
   exit 1
fi

# Keep the newest few builds for quick rollbacks, never the running one.
ls -1dt "$ROOT"/releases/build-* 2>/dev/null | tail -n +$((KEEP + 1)) | while read -r old; do
   [[ "$old" == "$DEST" ]] || rm -rf "$old"
done

echo "==> $BUILD ($COMMIT) is running"
