# Weather

A local-first weather dashboard (installable PWA) and an MCP server that gives AI agents the same data.

- **Web app** (`web/`): React + Vite + Tailwind. Settings, saved places, API keys and custom sources live in SQLite (WASM, stored in the browser's private file system).
- **Server** (`crates/weather-server`): Rust + Axum. REST API for the app, MCP for agents, and in production it also serves the built app.
- **Core** (`crates/weather-core`): the `WeatherProvider` trait, normalized types (exported to TypeScript), and the providers: Open-Meteo (weather, marine, tides), NWS (US alerts) and user-defined custom sources.

## Run it

Needs Rust (stable) and Node 20+.

```bash
npm install
npm run dev          # API on :8787, app on http://localhost:5173
```

Production build (one process serves app, API and MCP on http://127.0.0.1:8787):

```bash
npm run build && npm start
```

Other scripts: `npm test` (Rust tests + TS typecheck), `npm run types` (regenerate `web/src/types` from the Rust types).

## MCP server

| Transport | URL |
|---|---|
| Streamable HTTP (current spec, streams over SSE) | `http://127.0.0.1:8787/mcp` |
| Legacy HTTP+SSE (protocol 2024-11-05) | `http://127.0.0.1:8787/sse` |

Tools:

| Tool | Arguments | Returns |
|---|---|---|
| `get_current_weather` | `lat`, `lon`, `provider?` | current conditions |
| `get_severe_alerts` | `lat`, `lon` | active alerts; `coverage: false` outside the US |
| `get_marine_tides` | `lat`, `lon` | upcoming high/low tides, waves now, 24 h wave outlook; `coastal: false` inland |
| `get_forecast` | `lat`, `lon`, `hours?`, `days?`, `provider?` | hourly + daily forecast |
| `search_places` | `query`, `limit?` | places with coordinates |
| `list_providers` | – | provider ids for the `provider` argument |

All tools are read-only and return structured JSON with an output schema. Failures come back as tool errors with a reason and code (e.g. `[invalid_location]`).

Connect Claude Code:

```bash
claude mcp add --transport http weather http://127.0.0.1:8787/mcp
```

Other clients: point them at the URL above; use `/sse` for clients that only support the older SSE transport.

Custom sources configured in the app are available to agents too (as `custom:<id>`), even while the app is closed: the server keeps a copy in `data/custom-providers.json` (owner-only permissions).

## Configuration

| Variable | Default | |
|---|---|---|
| `HOST` / `PORT` | `127.0.0.1` / `8787` | Listen address. Keep it on loopback unless you know you need otherwise. |
| `ALLOWED_HOSTS` | – | Extra host names to accept (comma-separated). All requests whose `Host` or `Origin` isn't local are rejected, to stop other websites reaching the server via DNS rebinding. |
| `DATA_DIR` | `data` | Where the server keeps custom sources. |
| `WEB_DIST` | `web/dist` | Built app to serve. |
| `RUST_LOG` | `weather_server=info,tower_http=info` | Log filter. |

## Location and coastal data

- **My location** uses the browser's geolocation at city-level accuracy, only when you ask. The entry follows you: when the app opens or comes back to the foreground it updates (without prompting) if you've moved more than 1 km. Its name comes from OpenStreetMap Nominatim, which only ever receives the position rounded to ~1 km.
- **Tides and waves** appear only for locations on or near open water. Inland points get no marine data; large lakes get waves but no tides.

## Notes

- Place names for "my location": © OpenStreetMap contributors (Nominatim; at most 1 request/s, cached per ~1 km for a day).
- Tides are modeled from Open-Meteo sea-level data, not tide-station predictions. Not for navigation.
- Severe weather alerts come from the US National Weather Service only.
- Only one browser tab at a time can save settings; others run on a temporary in-memory copy and say so.
