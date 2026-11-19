# Deploying

The app runs as one small process (about 20 MB of memory) that serves the web app, the API and MCP. On a server it sits behind Caddy, which provides HTTPS. HTTPS is required: the app can't be installed on a phone, and "my location" doesn't work, on an insecure page.

It is set up to share the **kwaai** server (Oracle Ampere, Arm64, Ubuntu 24.04), which already runs Caddy. Any Arm64 Ubuntu server with Caddy works the same way.

| Piece | Where |
|---|---|
| Build | GitHub Actions builds every push to `master` and publishes a release (`build-<n>`) with the Arm64 server and the web app |
| Server process | `weather.service` (systemd), user `weather`, listening on `127.0.0.1:8787` only |
| Program | `/opt/weather/current` → `/opt/weather/releases/build-<n>` (the last three are kept) |
| Settings | `/etc/weather/weather.env` (owner token, domain, port) |
| Data | `/var/lib/weather` (the server's copy of your custom sources) |
| Web address | `/etc/caddy/sites/weather.caddy`, imported by `/etc/caddy/Caddyfile` |

## Who can do what

A server on the internet has an **owner token** (`OWNER_TOKEN`, created at set-up). Without it:

- anyone can use the dashboard, with the built-in sources (Open-Meteo, NWS);
- nobody can add, change or use custom sources, which hold your API keys;
- nobody can use MCP (`/mcp`, `/sse`).

With it, everything works as it does locally. Enter it in the app under **Settings → Server access** on each device you use. MCP clients send it as `Authorization: Bearer <token>`.

The server won't start with `ALLOWED_HOSTS` set (or a non-loopback `HOST`) but no `OWNER_TOKEN`.

## One-time set-up

### 1. kwaai's Caddy config must import other sites

kwaai's `deploy/setup-oracle.sh` rewrites `/etc/caddy/Caddyfile` on every kwaai deploy. It must end with this line, or each kwaai deploy drops the weather site:

```
import /etc/caddy/sites/*.caddy
```

The line has been added to kwaai's script (`preowned-finder/deploy/setup-oracle.sh`). Commit it there. `install.sh` below also adds it to the live Caddyfile if it's missing.

### 2. DNS

Add an **A record** `weather.stassen.org.za` pointing at the kwaai server's IP (`92.4.129.38`). Ports 80 and 443 are already open there. Check it from the Mac:

```bash
dig +short weather.stassen.org.za
```

### 3. A first release

Push to `master` (already done for the first build) and wait for the **CI** run to finish: GitHub → Actions. The `release` job publishes `build-<n>` under Releases.

### 4. Install

From the project folder on the Mac:

```bash
./deploy/push.sh ubuntu@92.4.129.38 weather.stassen.org.za
```

This copies `deploy/install.sh` and `deploy/update.sh` to the server and runs the set-up: user, folders, service, Caddy site, the newest release, and an HTTPS check. At the end it prints the **access token** once. Store it in your password manager. It is also in `/etc/weather/weather.env` on the server.

Then:

- open `https://weather.stassen.org.za`, go to **Settings → Server access** and enter the token;
- add your custom sources again if you use any (the server starts with none);
- connect Claude Code:

  ```bash
  claude mcp add --transport http weather https://weather.stassen.org.za/mcp --header "Authorization: Bearer <token>"
  ```

Running `push.sh` again is safe: the token and data are kept.

## Updating

By hand, after CI has published a new build:

```bash
ssh ubuntu@92.4.129.38 sudo weather-update
```

`weather-update` downloads the newest release, checks its checksum, switches to it and restarts. If the new build doesn't answer its health check, it switches back to the previous one. To install a specific build (also how you roll back): `sudo weather-update build-12`.

The running build is shown by `https://weather.stassen.org.za/api/health` (`build` is the commit).

### Automatic deploys (optional)

Every push to `master` can deploy itself once tests pass. On the server, create a key that may only run `weather-update`:

```bash
ssh-keygen -t ed25519 -f /tmp/actions_key -N "" -C "github-actions-weather-deploy"
echo "command=\"sudo /usr/local/bin/weather-update \\\"\$SSH_ORIGINAL_COMMAND\\\"\",no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-pty $(cat /tmp/actions_key.pub)" >> ~/.ssh/authorized_keys
cat /tmp/actions_key        # copy this, then:  rm /tmp/actions_key /tmp/actions_key.pub
```

The workflow sends the release tag as the SSH command. `weather-update` accepts only `build-<number>`.

From the Mac, record the server's host key: `ssh-keyscan -t ed25519 92.4.129.38`.

In GitHub → the weather repository → Settings → Secrets and variables → Actions:

| Kind | Name | Value |
|---|---|---|
| Secret | `DEPLOY_SSH_KEY` | the private key printed above |
| Secret | `DEPLOY_KNOWN_HOSTS` | the `ssh-keyscan` output |
| Variable | `DEPLOY_HOST` | `92.4.129.38` |
| Variable | `DEPLOY_USER` | `ubuntu` (the default) |

Without `DEPLOY_HOST` the deploy job is skipped. Delete it to pause automatic deploys.

## Day-to-day

```bash
systemctl status weather                       # is it running
sudo journalctl -u weather -f                  # follow the log
sudo weather-update                            # install the newest build
sudo weather-update build-12                   # install (or roll back to) a given build
sudo weather-token                             # new access token (devices and MCP clients need it again)
ls /opt/weather/releases                       # builds kept for rollback
```

Point Uptime Kuma at `https://weather.stassen.org.za/api/health` (HTTP, keyword `"ok"`).

## Notes

- **Backups:** the only server-side data is `/var/lib/weather/custom-providers.json`, the copy of your custom sources (with their API keys) that the app re-sends whenever you change them. Each browser keeps its own settings and saved places in its private storage.
- **Data sources:** Nominatim (place names for "my location") asks for at most one request per second. The server caches names per ~1 km for a day, which keeps a personal dashboard far below that.
- **Without the token**, a visitor's own custom sources stay in their browser and aren't used. The data source list shows only the built-in ones.
