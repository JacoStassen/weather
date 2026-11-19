//! Legacy MCP HTTP+SSE transport (protocol 2024-11-05), for clients that
//! haven't moved to Streamable HTTP yet. The official SDK no longer ships a
//! server for it, so this adapts it onto the SDK's generic sink/stream
//! transport:
//!
//! - `GET /sse` opens an event stream. The first event (`endpoint`) tells the
//!   client where to POST; server messages follow as `message` events.
//! - `POST /message?sessionId=…` delivers one JSON-RPC message from the client.

use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::{rejection::JsonRejection, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive},
        IntoResponse, Response, Sse,
    },
    routing::{get, post},
    Json, Router,
};
use futures::{stream, Stream, StreamExt};
use rmcp::{
    model::{ClientJsonRpcMessage, ServerJsonRpcMessage},
    ServiceExt,
};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::{sync::CancellationToken, sync::PollSender};

use super::WeatherMcp;
use crate::state::AppState;

type Sessions = Arc<Mutex<HashMap<String, mpsc::Sender<ClientJsonRpcMessage>>>>;

#[derive(Clone)]
struct SseState {
    app: AppState,
    sessions: Sessions,
    shutdown: CancellationToken,
}

/// Routes for `/sse` and `/message`, relative to where they're mounted.
pub fn router(app: AppState, shutdown: CancellationToken) -> Router {
    Router::new()
        .route("/sse", get(connect))
        .route("/message", post(message))
        .with_state(SseState {
            app,
            sessions: Arc::default(),
            shutdown,
        })
}

/// Removes the session when the client's event stream goes away, which
/// closes the session's inbox and stops its MCP service.
struct SessionGuard {
    sessions: Sessions,
    id: String,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.sessions.lock().expect("sessions lock").remove(&self.id);
        tracing::debug!(session = %self.id, "legacy SSE session closed");
    }
}

async fn connect(State(s): State<SseState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let (client_tx, client_rx) = mpsc::channel::<ClientJsonRpcMessage>(32);
    let (server_tx, server_rx) = mpsc::channel::<ServerJsonRpcMessage>(32);
    s.sessions.lock().expect("sessions lock").insert(id.clone(), client_tx);

    let handler = WeatherMcp::new(s.app.clone());
    let shutdown = s.shutdown.clone();
    let session = id.clone();
    tokio::spawn(async move {
        let transport = (PollSender::new(server_tx), ReceiverStream::new(client_rx));
        match handler.serve(transport).await {
            Ok(running) => {
                tokio::select! {
                    _ = running.waiting() => {}
                    _ = shutdown.cancelled() => {}
                }
            }
            Err(e) => tracing::debug!(%session, error = %e, "legacy SSE session ended before initialize"),
        }
    });

    let guard = SessionGuard {
        sessions: s.sessions.clone(),
        id: id.clone(),
    };
    let endpoint = Event::default().event("endpoint").data(format!("message?sessionId={id}"));
    let messages = ReceiverStream::new(server_rx).map(move |msg| {
        let _keep_alive_until_stream_drops = &guard;
        let data = serde_json::to_string(&msg).unwrap_or_else(|e| {
            json!({ "jsonrpc": "2.0", "error": { "code": -32603, "message": e.to_string() } }).to_string()
        });
        Ok(Event::default().event("message").data(data))
    });
    let stream = stream::once(async move { Ok(endpoint) })
        .chain(messages)
        .take_until(s.shutdown.cancelled_owned());

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[derive(Deserialize)]
struct MessageQuery {
    #[serde(rename = "sessionId")]
    session_id: String,
}

async fn message(
    State(s): State<SseState>,
    q: Result<Query<MessageQuery>, axum::extract::rejection::QueryRejection>,
    body: Result<Json<ClientJsonRpcMessage>, JsonRejection>,
) -> Response {
    let Ok(Query(q)) = q else {
        return (StatusCode::BAD_REQUEST, "missing sessionId").into_response();
    };
    let tx = s.sessions.lock().expect("sessions lock").get(&q.session_id).cloned();
    let Some(tx) = tx else {
        return (StatusCode::NOT_FOUND, "unknown or closed session").into_response();
    };
    let msg = match body {
        Ok(Json(m)) => m,
        Err(e) => return (StatusCode::BAD_REQUEST, e.body_text()).into_response(),
    };
    match tx.send(msg).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(_) => (StatusCode::GONE, "session closed").into_response(),
    }
}
