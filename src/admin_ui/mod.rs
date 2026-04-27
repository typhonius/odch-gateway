pub mod auth;

use axum::http::{header, StatusCode};
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::Router;
use rust_embed::Embed;

use crate::api;
use crate::state::AppState;

#[derive(Embed)]
#[folder = "admin-ui/"]
struct AdminAssets;

/// Build the admin UI router served on a separate port.
///
/// Routes:
///   POST /login         — authenticate, get session cookie
///   POST /logout        — clear session cookie
///   /api/v1/*           — same API endpoints, session-auth'd
///   /ws                 — WebSocket with session cookie auth
///   /*                  — static assets (SPA with index.html fallback)
pub fn build_admin_router(state: AppState) -> Router {
    // Rate limiter (reuse same config as main API)
    let rate_limit_config = state.config.rate_limit.clone();
    let requests_per_minute = rate_limit_config
        .map(|c| c.requests_per_minute)
        .unwrap_or(10);
    let limiter = api::rate_limit::RateLimiter::new(requests_per_minute);

    // Write endpoints (rate-limited)
    let write_routes = Router::new()
        .route("/chat/message", post(api::chat::send_message))
        .route("/users/:nick/kick", post(api::moderation::kick_user))
        .route("/users/:nick/ban", post(api::moderation::ban_user))
        .route("/users/:nick/ban", delete(api::moderation::unban_user))
        .route("/users/:nick/gag", post(api::moderation::gag_user))
        .route("/users/:nick/gag", delete(api::moderation::ungag_user))
        .route(
            "/commands/:name/execute",
            post(api::commands::execute_command),
        )
        .route("/webhooks", post(api::webhooks::create_webhook))
        .route("/webhooks/:id", put(api::webhooks::update_webhook))
        .route("/webhooks/:id", delete(api::webhooks::delete_webhook))
        .layer(middleware::from_fn_with_state(
            limiter,
            api::rate_limit::rate_limit_middleware,
        ));

    // Read endpoints
    let read_routes = Router::new()
        .route("/hub/info", get(api::hub::get_hub_info))
        .route("/hub/stats", get(api::hub::get_hub_stats))
        .route("/users", get(api::users::list_users))
        .route("/users/:nick", get(api::users::get_user))
        .route("/users/:nick/history", get(api::users::get_user_history))
        .route("/chat/history", get(api::chat::get_chat_history))
        .route("/commands", get(api::commands::list_commands))
        .route("/webhooks", get(api::webhooks::list_webhooks));

    // API routes with session auth
    let api_routes = Router::new()
        .merge(write_routes)
        .merge(read_routes)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));

    // WebSocket (session auth via cookie, not query param)
    let ws_route = Router::new()
        .route("/ws", get(api::websocket::ws_handler))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));

    // Public routes (no auth)
    let public_routes = Router::new()
        .route("/login", post(auth::login_handler))
        .route("/logout", post(auth::logout_handler));

    Router::new()
        .merge(public_routes)
        .nest("/api/v1", api_routes)
        .merge(ws_route)
        .fallback(serve_static)
        .with_state(state)
}

/// Serve embedded static files, with SPA fallback to index.html.
async fn serve_static(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try exact file match first
    if let Some(file) = AdminAssets::get(path) {
        let mime = mime_from_path(path);
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime)],
            file.data.to_vec(),
        )
            .into_response();
    }

    // SPA fallback: serve index.html for non-file paths
    if let Some(file) = AdminAssets::get("index.html") {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            file.data.to_vec(),
        )
            .into_response();
    }

    StatusCode::NOT_FOUND.into_response()
}

fn mime_from_path(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else {
        "application/octet-stream"
    }
}
