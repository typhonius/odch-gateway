use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use crate::error::AppError;
use crate::state::AppState;

/// Middleware that validates the X-API-Key header against configured API keys.
///
/// Extracts the AppState from request extensions and checks the API key.
/// Returns 401 Unauthorized if the key is missing or invalid.
pub async fn require_api_key(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let api_key = request
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok());

    match api_key {
        Some(key) if state.config.auth.api_keys.contains(&key.to_string()) => {
            Ok(next.run(request).await)
        }
        _ => Err(AppError::Unauthorized),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::EventBus;
    use crate::config::{AppConfig, AuthConfig, HubConfig, ServerConfig};
    use crate::state::{AppState, HubState};
    use crate::webhook::manager::WebhookManager;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::middleware;
    use axum::routing::get;
    use axum::Router;
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
                api_keys: vec!["valid-key-123".to_string()],
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

    async fn dummy_handler() -> &'static str {
        "ok"
    }

    fn test_router(state: AppState) -> Router {
        Router::new()
            .route("/protected", get(dummy_handler))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                require_api_key,
            ))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_valid_api_key() {
        let state = test_state();
        let app = test_router(state);

        let req = HttpRequest::builder()
            .uri("/protected")
            .header("X-API-Key", "valid-key-123")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_invalid_api_key() {
        let state = test_state();
        let app = test_router(state);

        let req = HttpRequest::builder()
            .uri("/protected")
            .header("X-API-Key", "wrong-key")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_missing_api_key() {
        let state = test_state();
        let app = test_router(state);

        let req = HttpRequest::builder()
            .uri("/protected")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
