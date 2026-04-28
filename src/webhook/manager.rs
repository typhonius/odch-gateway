use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

/// A registered webhook.
/// The `secret` field is excluded from serialization so it is never
/// leaked in API responses.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Webhook {
    pub id: String,
    pub url: String,
    #[serde(skip_serializing)]
    #[sqlx(default)]
    pub secret: String,
    pub events: String, // JSON array stored as text
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl Webhook {
    /// Parse the events JSON string into a Vec.
    pub fn event_list(&self) -> Vec<String> {
        serde_json::from_str(&self.events).unwrap_or_default()
    }
}

/// Request body for creating/updating a webhook.
#[derive(Debug, Deserialize)]
pub struct WebhookInput {
    pub url: String,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
}

fn default_enabled() -> bool {
    true
}

/// Database-backed webhook manager.
#[derive(Debug, Clone)]
pub struct WebhookManager {
    pool: Option<PgPool>,
    max_webhooks: usize,
}

impl WebhookManager {
    pub fn new(pool: Option<PgPool>, max_webhooks: usize) -> Self {
        Self { pool, max_webhooks }
    }

    fn db(&self) -> Result<&PgPool, AppError> {
        self.pool
            .as_ref()
            .ok_or_else(|| AppError::Internal("Database not configured".into()))
    }

    pub async fn list(&self) -> Vec<Webhook> {
        let Ok(pool) = self.db() else {
            return Vec::new();
        };
        sqlx::query_as::<_, Webhook>("SELECT id, url, secret, events, enabled, description, created_at FROM webhooks ORDER BY created_at")
            .fetch_all(pool)
            .await
            .unwrap_or_default()
    }

    pub async fn create(&self, input: WebhookInput) -> Result<Webhook, AppError> {
        let pool = self.db()?;

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM webhooks")
            .fetch_one(pool)
            .await?;
        if count.0 as usize >= self.max_webhooks {
            return Err(AppError::BadRequest(format!(
                "Maximum number of webhooks ({}) reached",
                self.max_webhooks
            )));
        }

        let id = Uuid::new_v4().to_string();
        let events_json = serde_json::to_string(&input.events).unwrap_or_else(|_| "[]".into());
        let secret = input.secret.unwrap_or_default();

        let webhook = sqlx::query_as::<_, Webhook>(
            "INSERT INTO webhooks (id, url, secret, events, enabled, description) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             RETURNING id, url, secret, events, enabled, description, created_at",
        )
        .bind(&id)
        .bind(&input.url)
        .bind(&secret)
        .bind(&events_json)
        .bind(input.enabled)
        .bind(&input.description)
        .fetch_one(pool)
        .await?;

        Ok(webhook)
    }

    pub async fn update(&self, id: &str, input: WebhookInput) -> Result<Webhook, AppError> {
        let pool = self.db()?;
        let events_json = serde_json::to_string(&input.events).unwrap_or_else(|_| "[]".into());

        // If no new secret provided, keep the existing one
        let webhook = if let Some(secret) = input.secret {
            sqlx::query_as::<_, Webhook>(
                "UPDATE webhooks SET url = $2, secret = $3, events = $4, enabled = $5, description = $6 \
                 WHERE id = $1 \
                 RETURNING id, url, secret, events, enabled, description, created_at",
            )
            .bind(id)
            .bind(&input.url)
            .bind(&secret)
            .bind(&events_json)
            .bind(input.enabled)
            .bind(&input.description)
            .fetch_optional(pool)
            .await?
        } else {
            sqlx::query_as::<_, Webhook>(
                "UPDATE webhooks SET url = $2, events = $3, enabled = $4, description = $5 \
                 WHERE id = $1 \
                 RETURNING id, url, secret, events, enabled, description, created_at",
            )
            .bind(id)
            .bind(&input.url)
            .bind(&events_json)
            .bind(input.enabled)
            .bind(&input.description)
            .fetch_optional(pool)
            .await?
        };

        webhook.ok_or_else(|| AppError::NotFound(format!("Webhook {} not found", id)))
    }

    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let pool = self.db()?;
        let result = sqlx::query("DELETE FROM webhooks WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!("Webhook {} not found", id)));
        }
        Ok(())
    }

    pub async fn get_matching(&self, event_type: &str) -> Vec<Webhook> {
        let Ok(pool) = self.db() else {
            return Vec::new();
        };
        // Get enabled webhooks where events is empty (all) or contains the event type
        let all = sqlx::query_as::<_, Webhook>(
            "SELECT id, url, secret, events, enabled, description, created_at \
             FROM webhooks WHERE enabled = true",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        all.into_iter()
            .filter(|w| {
                let events = w.event_list();
                events.is_empty() || events.iter().any(|e| e == event_type)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    // Tests require a PostgreSQL instance — run with TEST_DATABASE_URL set
}
