use axum::extract::{Path, State};
use axum::Json;

use crate::error::AppError;
use crate::state::AppState;
use crate::webhook::manager::WebhookInput;

/// GET /api/webhooks
///
/// List all registered webhooks.
pub async fn list_webhooks(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let webhooks = state.webhook_manager.list().await;
    Ok(Json(serde_json::json!({
        "webhooks": webhooks,
        "count": webhooks.len(),
    })))
}

/// POST /api/webhooks
///
/// Create a new webhook.
pub async fn create_webhook(
    State(state): State<AppState>,
    Json(body): Json<WebhookInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.url.is_empty() {
        return Err(AppError::BadRequest("URL is required".to_string()));
    }

    let webhook = state.webhook_manager.create(body).await?;

    Ok(Json(serde_json::json!({
        "webhook": webhook,
    })))
}

/// PUT /api/webhooks/:id
///
/// Update an existing webhook.
pub async fn update_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<WebhookInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.url.is_empty() {
        return Err(AppError::BadRequest("URL is required".to_string()));
    }

    let webhook = state.webhook_manager.update(&id, body).await?;

    Ok(Json(serde_json::json!({
        "webhook": webhook,
    })))
}

/// DELETE /api/webhooks/:id
///
/// Delete a webhook.
pub async fn delete_webhook(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.webhook_manager.delete(&id).await?;

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "id": id,
    })))
}
