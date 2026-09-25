//! Read-only HTTP API for the dashboard. It can watch the site but not
//! command it: control authority stays with the DSO link.
//!
//!   GET /api/snapshot   latest control cycle as JSON
//!   GET /api/plan       the plan in force, step by step (null without one)
//!   GET /api/reports    compliance reports of past dimmings (file names)
//!   GET /api/reports/{file}  one report as CSV
//!   GET /api/ws         WebSocket: {"type":"snapshot",...} and {"type":"frame",...}

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use tokio::sync::{broadcast, watch};
use tower_http::services::ServeDir;

use crate::ems::Ems;
use crate::snapshot::{FrameLog, Snapshot};

#[derive(Clone)]
struct Api {
    snapshot: watch::Receiver<Snapshot>,
    frames: broadcast::Sender<FrameLog>,
    ems: Option<Arc<Mutex<Ems>>>,
    reports: Arc<PathBuf>,
}

pub fn router(
    snapshot: watch::Receiver<Snapshot>,
    frames: broadcast::Sender<FrameLog>,
    web_root: Option<PathBuf>,
    ems: Option<Arc<Mutex<Ems>>>,
    reports_dir: PathBuf,
) -> Router {
    let app = Router::new()
        .route("/api/snapshot", get(get_snapshot))
        .route("/api/plan", get(get_plan))
        .route("/api/reports", get(get_reports))
        .route("/api/reports/{file}", get(get_report))
        .route("/api/ws", get(ws))
        .with_state(Api { snapshot, frames, ems, reports: Arc::new(reports_dir) });
    match web_root {
        Some(dir) => app.fallback_service(ServeDir::new(dir)),
        None => app,
    }
}

async fn get_snapshot(State(api): State<Api>) -> Json<Snapshot> {
    Json(api.snapshot.borrow().clone())
}

async fn get_plan(State(api): State<Api>) -> Json<serde_json::Value> {
    Json(api.ems.as_ref().map_or(serde_json::Value::Null, |e| e.lock().unwrap().plan_json()))
}

async fn get_reports(State(api): State<Api>) -> Json<Vec<crate::reports::ReportEntry>> {
    Json(crate::reports::list(Path::new(api.reports.as_ref())))
}

async fn get_report(State(api): State<Api>, UrlPath(file): UrlPath<String>) -> impl IntoResponse {
    match crate::reports::read(Path::new(api.reports.as_ref()), &file) {
        Some(csv) => (StatusCode::OK, [(header::CONTENT_TYPE, "text/csv; charset=utf-8")], csv).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
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
