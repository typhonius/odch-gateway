pub mod auth;
pub mod chat;
pub mod commands;
pub mod hub;
pub mod moderation;
pub mod rate_limit;
pub mod users;
pub mod webhooks;
pub mod websocket;

use axum::middleware;
use axum::routing::{delete, get, post, put};
use axum::Json;
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

/// Build the complete axum Router with all routes and middleware.
pub fn build_router(state: AppState) -> Router {
    // CORS configuration: deny all origins when none configured
    let cors = if state.config.server.cors_origins.is_empty() {
        CorsLayer::new().allow_methods(Any).allow_headers(Any)
    } else {
        let origins: Vec<_> = state
            .config
            .server
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
    };

    // Rate limiter for write endpoints
    let rate_limit_config = state.config.rate_limit.clone();
    let requests_per_minute = rate_limit_config
        .map(|c| c.requests_per_minute)
        .unwrap_or(10);
    let limiter = rate_limit::RateLimiter::new(requests_per_minute);

    // Write endpoints (rate-limited)
    let write_routes = Router::new()
        .route("/chat/message", post(chat::send_message))
        .route("/users/:nick/kick", post(moderation::kick_user))
        .route("/users/:nick/ban", post(moderation::ban_user))
        .route("/users/:nick/ban", delete(moderation::unban_user))
        .route("/users/:nick/gag", post(moderation::gag_user))
        .route("/users/:nick/gag", delete(moderation::ungag_user))
        .route("/commands/:name/execute", post(commands::execute_command))
        .route("/webhooks", post(webhooks::create_webhook))
        .route("/webhooks/:id", put(webhooks::update_webhook))
        .route("/webhooks/:id", delete(webhooks::delete_webhook))
        .layer(middleware::from_fn_with_state(
            limiter,
            rate_limit::rate_limit_middleware,
        ));

    // Read endpoints (no rate limit)
    let read_routes = Router::new()
        // Hub endpoints
        .route("/hub/info", get(hub::get_hub_info))
        .route("/hub/stats", get(hub::get_hub_stats))
        // User endpoints
        .route("/users", get(users::list_users))
        .route("/users/:nick", get(users::get_user))
        .route("/users/:nick/history", get(users::get_user_history))
        // Chat endpoints
        .route("/chat/history", get(chat::get_chat_history))
        // Command endpoints
        .route("/commands", get(commands::list_commands))
        // Webhook endpoints (read-only)
        .route("/webhooks", get(webhooks::list_webhooks));

    // Combine all API routes and apply auth middleware
    let api_routes =
        Router::new()
            .merge(write_routes)
            .merge(read_routes)
            .layer(middleware::from_fn_with_state(
                state.clone(),
                auth::require_api_key,
            ));

    // WebSocket (uses query param auth, not middleware)
    let ws_route = Router::new().route("/ws", get(websocket::ws_handler));

    // Health check (no auth)
    let health_route = Router::new().route("/health", get(health_check));

    Router::new()
        .nest("/api", api_routes)
        .merge(ws_route)
        .merge(health_route)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state)
}

/// GET /health
///
/// Simple health check endpoint. No authentication required.
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::EventBus;
    use crate::config::{AppConfig, AuthConfig, HubConfig, ServerConfig};
    use crate::state::{AppState, HubState};
    use crate::webhook::manager::WebhookManager;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn test_state() -> AppState {
        let config = AppConfig {
            server: ServerConfig {
                bind_address: "127.0.0.1:8080".to_string(),
                cors_origins: vec![],
            },
            hub: HubConfig {
                host: "localhost".to_string(),
                port: 411,
                nickname: "test".to_string(),
                description: "test".to_string(),
                email: String::new(),
                share_size: 0,
                speed: "LAN(T1)".to_string(),
                password: String::new(),
                reconnect_delay_secs: 5,
                max_reconnect_delay_secs: 300,
            },
            admin: None,
            database: None,
            auth: AuthConfig {
                api_keys: vec!["test-key".to_string()],
            },
            webhook: None,
            rate_limit: None,
        };
        let (nmdc_tx, _) = tokio::sync::mpsc::channel(1);
        let (admin_tx, _) = tokio::sync::mpsc::channel(1);
        AppState {
            config: Arc::new(config),
            event_bus: Arc::new(EventBus::new(16)),
            hub_state: Arc::new(HubState::new()),
            nmdc_tx: Arc::new(nmdc_tx),
            admin_tx: Arc::new(admin_tx),
            db_pool: None,
            webhook_manager: Arc::new(WebhookManager::in_memory(10)),
        }
    }

    #[tokio::test]
    async fn test_health_check() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_requires_auth() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/hub/info")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_api_with_valid_key() {
        let state = test_state();
        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/hub/info")
            .header("X-API-Key", "test-key")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_users_list() {
        let state = test_state();

        // Add a user to state
        state.hub_state.users.write().await.insert(
            "TestUser".to_string(),
            crate::state::HubUser {
                nick: "TestUser".to_string(),
                description: "Test".to_string(),
                speed: "LAN(T1)".to_string(),
                email: "test@example.com".to_string(),
                share: 1024,
                is_op: false,
            },
        );

        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/users")
            .header("X-API-Key", "test-key")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_user_by_nick() {
        let state = test_state();

        // Add a user to state
        state.hub_state.users.write().await.insert(
            "TestUser".to_string(),
            crate::state::HubUser {
                nick: "TestUser".to_string(),
                description: "Test".to_string(),
                speed: "LAN(T1)".to_string(),
                email: "test@example.com".to_string(),
                share: 1024,
                is_op: false,
            },
        );

        let app = build_router(state);

        let req = Request::builder()
            .uri("/api/users/TestUser")
            .header("X-API-Key", "test-key")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_webhook_crud() {
        let state = test_state();
        let app = build_router(state);

        // Create webhook
        let body = serde_json::json!({
            "url": "https://example.com/hook",
            "events": ["Chat"],
            "description": "Test hook"
        });

        let req = Request::builder()
            .method("POST")
            .uri("/api/webhooks")
            .header("X-API-Key", "test-key")
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
