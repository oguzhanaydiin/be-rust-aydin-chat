use std::collections::HashMap;

use tokio::sync::{mpsc::UnboundedSender, RwLock};

use crate::models::PendingMessage;

#[derive(Clone)]
pub struct UserConnection {
    pub connection_id: String,
    pub tx: UnboundedSender<String>,
}

pub struct AppState {
    pub db: mongodb::Database,
    pub jwt_secret: String,
    pub mailboxes: RwLock<HashMap<String, Vec<PendingMessage>>>,
    pub message_reactions: RwLock<HashMap<String, Vec<String>>>,
    pub online_users: RwLock<HashMap<String, Vec<UserConnection>>>,
}

impl AppState {
    pub async fn register_connection(
        &self,
        user_id: &str,
        connection_id: String,
        tx: UnboundedSender<String>,
    ) {
        let mut users = self.online_users.write().await;
        users
            .entry(user_id.to_string())
            .or_default()
            .push(UserConnection { connection_id, tx });
    }

    pub async fn unregister_connection(&self, user_id: &str, connection_id: &str) {
        let mut users = self.online_users.write().await;
        if let Some(connections) = users.get_mut(user_id) {
            connections.retain(|conn| conn.connection_id != connection_id);
            if connections.is_empty() {
                users.remove(user_id);
            }
        }
    }

    pub async fn online_user_ids(&self) -> Vec<String> {
        let users = self.online_users.read().await;
        users.keys().cloned().collect()
    }

    pub async fn queue_message(&self, message: PendingMessage) {
        {
            let mut reactions = self.message_reactions.write().await;
            reactions
                .entry(message.id.clone())
                .or_insert_with(|| message.hearted_by.clone());
        }

        let mut mailboxes = self.mailboxes.write().await;
        mailboxes
            .entry(message.to_username.clone())
            .or_default()
            .push(message);
    }

    pub async fn get_inbox(&self, user_id: &str) -> Vec<PendingMessage> {
        let mailboxes = self.mailboxes.read().await;
        let mut messages = mailboxes.get(user_id).cloned().unwrap_or_default();
        drop(mailboxes);

        let reactions = self.message_reactions.read().await;
        messages.iter_mut().for_each(|msg| {
            if let Some(hearted_by) = reactions.get(&msg.id) {
                msg.hearted_by = hearted_by.clone();
            }
        });

        messages
    }

    pub async fn toggle_message_heart(&self, message_id: &str, by_username: &str) -> Vec<String> {
        let normalized_by = by_username.trim().to_lowercase();
        let normalized_message_id = message_id.trim().to_string();

        if normalized_by.is_empty() || normalized_message_id.is_empty() {
            return Vec::new();
        }

        let next_hearted_by = {
            let mut reactions = self.message_reactions.write().await;
            let entry = reactions.entry(normalized_message_id.clone()).or_default();
            if let Some(index) = entry.iter().position(|username| username == &normalized_by) {
                entry.remove(index);
            } else {
                entry.push(normalized_by.clone());
            }

            entry.clone()
        };

        let mut mailboxes = self.mailboxes.write().await;
        mailboxes.values_mut().for_each(|messages| {
            messages.iter_mut().for_each(|msg| {
                if msg.id == normalized_message_id {
                    msg.hearted_by = next_hearted_by.clone();
                }
            });
        });

        next_hearted_by
    }

    pub async fn ack_messages(&self, user_id: &str, message_ids: &[String]) -> usize {
        if message_ids.is_empty() {
            return 0;
        }

        let message_ids: std::collections::HashSet<&String> = message_ids.iter().collect();
        let mut mailboxes = self.mailboxes.write().await;
        let Some(messages) = mailboxes.get_mut(user_id) else {
            return 0;
        };

        let before = messages.len();
        messages.retain(|msg| !message_ids.contains(&msg.id));
        before.saturating_sub(messages.len())
    }

    pub async fn dispatch_to_user(&self, user_id: &str, payload: &str) -> usize {
        let mut users = self.online_users.write().await;
        let Some(connections) = users.get_mut(user_id) else {
            return 0;
        };

        let mut delivered = 0usize;
        connections.retain(|conn| {
            let ok = conn.tx.send(payload.to_string()).is_ok();
            if ok {
                delivered += 1;
            }
            ok
        });

        if connections.is_empty() {
            users.remove(user_id);
        }

        delivered
    }

    pub async fn broadcast_to_all_online(&self, payload: &str) {
        let mut users = self.online_users.write().await;
        users.retain(|_, connections| {
            connections.retain(|conn| conn.tx.send(payload.to_string()).is_ok());
            !connections.is_empty()
        });
    }
}
