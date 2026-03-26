use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

/// A registered webhook.
///
/// The `secret` field is excluded from serialization so it is never
/// leaked in API responses. Disk persistence uses [`WebhookStorage`]
/// which includes the secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Webhook {
    pub id: String,
    pub url: String,
    #[serde(skip_serializing, default)]
    pub secret: String,
    pub events: Vec<String>,
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default = "chrono::Utc::now")]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Private struct used only for disk persistence that includes the secret.
#[derive(Serialize, Deserialize)]
struct WebhookStorage {
    id: String,
    url: String,
    secret: String,
    events: Vec<String>,
    enabled: bool,
    #[serde(default)]
    description: String,
    #[serde(default = "chrono::Utc::now")]
    created_at: chrono::DateTime<chrono::Utc>,
}

impl From<&Webhook> for WebhookStorage {
    fn from(w: &Webhook) -> Self {
        Self {
            id: w.id.clone(),
            url: w.url.clone(),
            secret: w.secret.clone(),
            events: w.events.clone(),
            enabled: w.enabled,
            description: w.description.clone(),
            created_at: w.created_at,
        }
    }
}

impl From<WebhookStorage> for Webhook {
    fn from(s: WebhookStorage) -> Self {
        Self {
            id: s.id,
            url: s.url,
            secret: s.secret,
            events: s.events,
            enabled: s.enabled,
            description: s.description,
            created_at: s.created_at,
        }
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

/// JSON file-backed webhook storage.
#[derive(Debug, Clone)]
pub struct WebhookManager {
    webhooks: Arc<RwLock<HashMap<String, Webhook>>>,
    storage_path: PathBuf,
    max_webhooks: usize,
}

impl WebhookManager {
    /// Create a new manager, loading existing webhooks from disk if available.
    pub fn new(storage_path: &str, max_webhooks: usize) -> Self {
        let path = PathBuf::from(storage_path);
        let webhooks = Self::load_from_disk(&path).unwrap_or_default();

        Self {
            webhooks: Arc::new(RwLock::new(webhooks)),
            storage_path: path,
            max_webhooks,
        }
    }

    /// Create a manager that does not persist to disk (for testing).
    pub fn in_memory(max_webhooks: usize) -> Self {
        Self {
            webhooks: Arc::new(RwLock::new(HashMap::new())),
            storage_path: PathBuf::new(),
            max_webhooks,
        }
    }

    fn load_from_disk(path: &Path) -> Option<HashMap<String, Webhook>> {
        let data = std::fs::read_to_string(path).ok()?;
        let hooks: Vec<WebhookStorage> = serde_json::from_str(&data).ok()?;
        let map = hooks
            .into_iter()
            .map(|s| {
                let w: Webhook = s.into();
                (w.id.clone(), w)
            })
            .collect();
        Some(map)
    }

    async fn persist(&self) -> Result<(), AppError> {
        if self.storage_path.as_os_str().is_empty() {
            return Ok(());
        }
        let hooks = self.webhooks.read().await;
        let list: Vec<WebhookStorage> = hooks.values().map(WebhookStorage::from).collect();
        let json = serde_json::to_string_pretty(&list)
            .map_err(|e| AppError::Internal(format!("Serialize webhooks: {e}")))?;
        // Write atomically via temp file
        let tmp = self.storage_path.with_extension("tmp");
        tokio::fs::write(&tmp, json.as_bytes())
            .await
            .map_err(|e| AppError::Internal(format!("Write webhook file: {e}")))?;
        tokio::fs::rename(&tmp, &self.storage_path)
            .await
            .map_err(|e| AppError::Internal(format!("Rename webhook file: {e}")))?;
        Ok(())
    }

    /// List all webhooks.
    pub async fn list(&self) -> Vec<Webhook> {
        let hooks = self.webhooks.read().await;
        hooks.values().cloned().collect()
    }

    /// Get a webhook by ID.
    pub async fn get(&self, id: &str) -> Option<Webhook> {
        let hooks = self.webhooks.read().await;
        hooks.get(id).cloned()
    }

    /// Create a new webhook.
    pub async fn create(&self, input: WebhookInput) -> Result<Webhook, AppError> {
        let mut hooks = self.webhooks.write().await;

        if hooks.len() >= self.max_webhooks {
            return Err(AppError::BadRequest(format!(
                "Maximum number of webhooks ({}) reached",
                self.max_webhooks
            )));
        }

        let webhook = Webhook {
            id: Uuid::new_v4().to_string(),
            url: input.url,
            secret: input.secret.unwrap_or_default(),
            events: input.events,
            enabled: input.enabled,
            description: input.description,
            created_at: chrono::Utc::now(),
        };

        hooks.insert(webhook.id.clone(), webhook.clone());
        drop(hooks);
        self.persist().await?;
        Ok(webhook)
    }

    /// Update an existing webhook.
    pub async fn update(&self, id: &str, input: WebhookInput) -> Result<Webhook, AppError> {
        let mut hooks = self.webhooks.write().await;

        let existing = hooks
            .get(id)
            .ok_or_else(|| AppError::NotFound(format!("Webhook {} not found", id)))?;

        let webhook = Webhook {
            id: id.to_string(),
            url: input.url,
            secret: input.secret.unwrap_or_else(|| existing.secret.clone()),
            events: input.events,
            enabled: input.enabled,
            description: input.description,
            created_at: existing.created_at,
        };

        hooks.insert(id.to_string(), webhook.clone());
        drop(hooks);
        self.persist().await?;
        Ok(webhook)
    }

    /// Delete a webhook.
    pub async fn delete(&self, id: &str) -> Result<(), AppError> {
        let mut hooks = self.webhooks.write().await;
        if hooks.remove(id).is_none() {
            return Err(AppError::NotFound(format!("Webhook {} not found", id)));
        }
        drop(hooks);
        self.persist().await?;
        Ok(())
    }

    /// Get all enabled webhooks that subscribe to a given event type.
    pub async fn get_matching(&self, event_type: &str) -> Vec<Webhook> {
        let hooks = self.webhooks.read().await;
        hooks
            .values()
            .filter(|w| {
                w.enabled && (w.events.is_empty() || w.events.iter().any(|e| e == event_type))
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_list() {
        let mgr = WebhookManager::in_memory(10);
        let input = WebhookInput {
            url: "https://example.com/hook".to_string(),
            secret: Some("s3cret".to_string()),
            events: vec!["Chat".to_string()],
            enabled: true,
            description: "Test hook".to_string(),
        };

        let hook = mgr.create(input).await.unwrap();
        assert_eq!(hook.url, "https://example.com/hook");
        assert!(!hook.id.is_empty());

        let all = mgr.list().await;
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn test_update() {
        let mgr = WebhookManager::in_memory(10);
        let input = WebhookInput {
            url: "https://example.com/hook".to_string(),
            secret: Some("s3cret".to_string()),
            events: vec!["Chat".to_string()],
            enabled: true,
            description: "Original".to_string(),
        };
        let hook = mgr.create(input).await.unwrap();

        let update = WebhookInput {
            url: "https://example.com/hook2".to_string(),
            secret: None, // Should keep existing secret
            events: vec!["UserJoin".to_string()],
            enabled: false,
            description: "Updated".to_string(),
        };
        let updated = mgr.update(&hook.id, update).await.unwrap();
        assert_eq!(updated.url, "https://example.com/hook2");
        assert_eq!(updated.secret, "s3cret"); // Preserved
        assert!(!updated.enabled);
    }

    #[tokio::test]
    async fn test_delete() {
        let mgr = WebhookManager::in_memory(10);
        let input = WebhookInput {
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec![],
            enabled: true,
            description: String::new(),
        };
        let hook = mgr.create(input).await.unwrap();
        mgr.delete(&hook.id).await.unwrap();
        assert!(mgr.list().await.is_empty());
    }

    #[tokio::test]
    async fn test_delete_not_found() {
        let mgr = WebhookManager::in_memory(10);
        let result = mgr.delete("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_max_webhooks() {
        let mgr = WebhookManager::in_memory(2);
        for _ in 0..2 {
            let input = WebhookInput {
                url: "https://example.com/hook".to_string(),
                secret: None,
                events: vec![],
                enabled: true,
                description: String::new(),
            };
            mgr.create(input).await.unwrap();
        }
        let input = WebhookInput {
            url: "https://example.com/hook3".to_string(),
            secret: None,
            events: vec![],
            enabled: true,
            description: String::new(),
        };
        let result = mgr.create(input).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_matching() {
        let mgr = WebhookManager::in_memory(10);

        // Hook that listens to Chat events
        let input1 = WebhookInput {
            url: "https://example.com/chat".to_string(),
            secret: None,
            events: vec!["Chat".to_string()],
            enabled: true,
            description: String::new(),
        };
        mgr.create(input1).await.unwrap();

        // Hook that listens to all events (empty filter)
        let input2 = WebhookInput {
            url: "https://example.com/all".to_string(),
            secret: None,
            events: vec![],
            enabled: true,
            description: String::new(),
        };
        mgr.create(input2).await.unwrap();

        // Disabled hook for Chat
        let input3 = WebhookInput {
            url: "https://example.com/disabled".to_string(),
            secret: None,
            events: vec!["Chat".to_string()],
            enabled: false,
            description: String::new(),
        };
        mgr.create(input3).await.unwrap();

        let matching = mgr.get_matching("Chat").await;
        assert_eq!(matching.len(), 2);

        let matching_join = mgr.get_matching("UserJoin").await;
        assert_eq!(matching_join.len(), 1); // Only the "all" hook
    }
}
