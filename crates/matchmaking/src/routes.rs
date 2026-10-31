//! HTTP 路由

use axum::{Router, routing::{get, post}, Json, extract::State};
use serde::{Deserialize, Serialize};
use crate::store::MatchStore;
use crate::queue::MatchQueue;

#[derive(Clone)]
pub struct AppState {
    pub store: MatchStore,
    pub queue: MatchQueue,
}

pub fn build_router() -> Router {
    let state = AppState {
        store: MatchStore::new(),
        queue: MatchQueue::new(),
    };
    
    Router::new()
        .route("/health", get(health))
        .route("/queue/join", post(join_queue))
        .route("/queue/leave", post(leave_queue))
        .route("/match/status", get(match_status))
        .with_state(state)
}

#[derive(Serialize)]
struct Health {
    ok: bool,
}

async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

#[derive(Deserialize)]
struct JoinQueueRequest {
    player_id: String,
    hero_id: u32,
    mmr: u32,
}

#[derive(Serialize)]
struct JoinQueueResponse {
    success: bool,
    queue_position: Option<u32>,
    estimated_wait: Option<u32>,
}

async fn join_queue(
    State(state): State<AppState>,
    Json(req): Json<JoinQueueRequest>,
) -> Json<JoinQueueResponse> {
    let position = state.queue.join(req.player_id, req.hero_id, req.mmr).await;
    Json(JoinQueueResponse {
        success: true,
        queue_position: Some(position),
        estimated_wait: Some(position * 30), // 粗略估计
    })
}

#[derive(Deserialize)]
struct LeaveQueueRequest {
    player_id: String,
}

#[derive(Serialize)]
struct LeaveQueueResponse {
    success: bool,
}

async fn leave_queue(
    State(state): State<AppState>,
    Json(req): Json<LeaveQueueRequest>,
) -> Json<LeaveQueueResponse> {
    state.queue.leave(&req.player_id).await;
    Json(LeaveQueueResponse { success: true })
}

#[derive(Serialize)]
struct MatchStatusResponse {
    in_queue: bool,
    match_id: Option<u64>,
    server_address: Option<String>,
}

async fn match_status(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<MatchStatusResponse> {
    let player_id = params.get("player_id").cloned().unwrap_or_default();
    let (in_queue, match_id, server_address) = state.queue.get_status(&player_id).await;
    
    Json(MatchStatusResponse {
        in_queue,
        match_id,
        server_address,
    })
}