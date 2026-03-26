use crate::event::HubEvent;
use tokio::sync::broadcast;

/// Event bus using tokio broadcast channel.
/// Producers: NMDC client, admin client.
/// Consumers: WebSocket handlers, webhook dispatcher.
#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<HubEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn publish(&self, event: HubEvent) {
        // Ignore error if no receivers
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HubEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_publish_subscribe() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(HubEvent::Chat {
            nick: "Alice".to_string(),
            message: "Hello".to_string(),
            timestamp: Utc::now(),
        });

        let event = rx.recv().await.unwrap();
        match event {
            HubEvent::Chat { nick, message, .. } => {
                assert_eq!(nick, "Alice");
                assert_eq!(message, "Hello");
            }
            _ => panic!("Expected Chat event"),
        }
    }

    #[tokio::test]
    async fn test_no_receivers() {
        let bus = EventBus::new(16);
        // Should not panic even with no subscribers
        bus.publish(HubEvent::UserJoin {
            nick: "Bob".to_string(),
            timestamp: Utc::now(),
        });
    }
}
