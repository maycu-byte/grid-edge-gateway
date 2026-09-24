//! Read-only HTTP API for the dashboard. It can watch the site but not
//! command it: control authority stays with the DSO link.
//!
//!   GET /api/snapshot   latest control cycle as JSON
//!   GET /api/ws         WebSocket: {"type":"snapshot",...} and {"type":"frame",...}

use std::path::PathBuf;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use tokio::sync::{broadcast, watch};
use tower_http::services::ServeDir;

use crate::snapshot::{FrameLog, Snapshot};

#[derive(Clone)]
struct Api {
    snapshot: watch::Receiver<Snapshot>,
    frames: broadcast::Sender<FrameLog>,
}

pub fn router(
    snapshot: watch::Receiver<Snapshot>,
    frames: broadcast::Sender<FrameLog>,
    web_root: Option<PathBuf>,
) -> Router {
    let app = Router::new()
        .route("/api/snapshot", get(get_snapshot))
        .route("/api/ws", get(ws))
        .with_state(Api { snapshot, frames });
    match web_root {
        Some(dir) => app.fallback_service(ServeDir::new(dir)),
        None => app,
    }
}

async fn get_snapshot(State(api): State<Api>) -> Json<Snapshot> {
    Json(api.snapshot.borrow().clone())
}

async fn ws(upgrade: WebSocketUpgrade, State(api): State<Api>) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| stream(socket, api))
}

async fn stream(mut socket: WebSocket, api: Api) {
    let mut snapshot = api.snapshot.clone();
    let mut frames = api.frames.subscribe();
    loop {
        let msg = tokio::select! {
            changed = snapshot.changed() => {
                if changed.is_err() { return; }
                let s = snapshot.borrow_and_update().clone();
                json!({ "type": "snapshot", "data": s })
            }
            frame = frames.recv() => match frame {
                Ok(f) => json!({ "type": "frame", "data": f }),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(_)) => continue,
                _ => return,
            },
        };
        if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
            return;
        }
    }
}
