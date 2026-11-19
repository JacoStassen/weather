mod auth;
mod custom_store;
mod error;
mod guard;
mod mcp;
mod rest;
mod service;
mod state;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::Context;
use axum::{middleware, Router};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;
use weather_core::{
    http::default_client,
    providers::{open_meteo, NwsProvider, OpenMeteoProvider},
    ProviderRegistry,
};

use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "weather_server=info,tower_http=info".into()),
        )
        .init();

    let http = default_client();
    let mut registry = ProviderRegistry::new().with_fallback(open_meteo::ID);
    registry.register(Arc::new(OpenMeteoProvider::new(http.clone())));
    registry.register(Arc::new(NwsProvider::new(http.clone())));

    let data_dir = PathBuf::from(std::env::var("DATA_DIR").unwrap_or_else(|_| "data".into()));
    let mut state = AppState::new(registry, http, Some(data_dir));
    custom_store::load_into(&state);

    let shutdown = CancellationToken::new();
    let allowed = guard::AllowedHosts::from_env();
    let token = auth::OwnerToken::from_env()?;
    state.owner_token_required = token.is_set();

    // Loopback only by default: this server holds API keys and exposes MCP tools.
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .context("invalid HOST/PORT")?;

    // Reachable by others (a public name, or a non-loopback address) means
    // strangers could rewrite custom sources and use MCP: insist on a token.
    let exposed = !allowed.extra_hosts().is_empty() || !addr.ip().is_loopback();
    if exposed && !token.is_set() {
        anyhow::bail!(
            "ALLOWED_HOSTS or a non-loopback HOST makes this server reachable by others; \
             set OWNER_TOKEN too (e.g. OWNER_TOKEN=$(openssl rand -hex 24))"
        );
    }
    if token.is_set() {
        tracing::info!("owner token set: custom sources and MCP need it");
    }

    // MCP over Streamable HTTP. Host/Origin checks are done by our guard
    // layer for every route; the SDK's own Host check is given the same list.
    let mcp_state = state.clone();
    let mcp_http = StreamableHttpService::new(
        move || Ok(mcp::WeatherMcp::new(mcp_state.clone())),
        LocalSessionManager::default().into(),
        {
            let mut config = StreamableHttpServerConfig::default();
            config.allowed_hosts.extend(allowed.extra_hosts().iter().cloned());
            config.cancellation_token = shutdown.child_token();
            config
        },
    );

    // Everything MCP is owner-only (when a token is set).
    let mcp_routes = Router::new()
        .nest_service("/mcp", mcp_http)
        .merge(mcp::legacy_sse::router(state.clone(), shutdown.child_token()))
        .layer(middleware::from_fn(auth::require_owner));

    let mut app = Router::new()
        .nest("/api", rest::router())
        .merge(mcp_routes.with_state::<AppState>(()));

    // In production the server also serves the built PWA, so the app, the API
    // and the MCP endpoint share one origin. In development Vite serves the app
    // and proxies /api to this server.
    let dist = PathBuf::from(std::env::var("WEB_DIST").unwrap_or_else(|_| "web/dist".into()));
    if dist.join("index.html").exists() {
        tracing::info!("serving web app from {}", dist.display());
        app = app.fallback_service(
            // `fallback` (not `not_found_service`) so client-side routes like
            // /settings get the app shell with 200, not 404.
            ServeDir::new(&dist).fallback(ServeFile::new(dist.join("index.html"))),
        );
    }

    let app = app
        .with_state(state)
        .layer(middleware::from_fn_with_state(token, auth::identify))
        .layer(middleware::from_fn_with_state(allowed, guard::check))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!("listening on http://{addr}");
    tracing::info!("MCP: http://{addr}/mcp (Streamable HTTP), http://{addr}/sse (legacy SSE)");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            // Ends open MCP event streams, which would otherwise hold shutdown.
            shutdown.cancel();
        })
        .await?;
    Ok(())
}
