use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::config::AdminUiConfig;
use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// POST /login — verify credentials, return JWT in httpOnly cookie.
pub async fn login_handler(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let ui_config = state
        .config
        .admin_ui
        .as_ref()
        .ok_or_else(|| AppError::Internal("Admin UI not configured".to_string()))?;

    // Verify username
    if body.username != ui_config.username {
        return Err(AppError::Unauthorized);
    }

    // Verify password against bcrypt hash
    let valid = bcrypt::verify(&body.password, &ui_config.password_hash)
        .map_err(|e| AppError::Internal(format!("Password verification error: {e}")))?;

    if !valid {
        return Err(AppError::Unauthorized);
    }

    // Create JWT
    let token = create_token(&body.username, ui_config)?;

    // Set httpOnly cookie
    let cookie = format!(
        "session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        token,
        ui_config.session_expiry_hours * 3600
    );

    let mut response = Json(serde_json::json!({"status": "ok"})).into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, cookie.parse().unwrap());
    Ok(response)
}

/// POST /logout — clear session cookie.
pub async fn logout_handler() -> Response {
    let cookie = "session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0";
    let mut response = Json(serde_json::json!({"status": "ok"})).into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, cookie.parse().unwrap());
    response
}

/// Middleware: validate JWT from cookie on protected routes.
pub async fn require_session(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let ui_config = state
        .config
        .admin_ui
        .as_ref()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Extract session cookie
    let token = request
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                let c = c.trim();
                c.strip_prefix("session=")
            })
        })
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Validate JWT
    let _claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(ui_config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?;

    Ok(next.run(request).await)
}

fn create_token(username: &str, config: &AdminUiConfig) -> Result<String, AppError> {
    let expiry = chrono::Utc::now()
        + chrono::Duration::hours(config.session_expiry_hours as i64);

    let claims = Claims {
        sub: username.to_string(),
        exp: expiry.timestamp() as usize,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("JWT creation error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_validate_token() {
        let config = AdminUiConfig {
            bind_address: "127.0.0.1:3001".to_string(),
            username: "admin".to_string(),
            password_hash: String::new(),
            session_expiry_hours: 8,
            jwt_secret: "test-secret-key-for-jwt-signing".to_string(),
        };

        let token = create_token("admin", &config).unwrap();
        assert!(!token.is_empty());

        let claims = decode::<Claims>(
            &token,
            &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .unwrap();

        assert_eq!(claims.claims.sub, "admin");
    }
}
