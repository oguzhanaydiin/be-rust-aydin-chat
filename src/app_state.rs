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
    pub message_reactions: RwLock<HashMap<String, HashMap<String, Vec<String>>>>,
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
                .or_insert_with(|| message.reactions.clone());
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
            if let Some(message_reactions) = reactions.get(&msg.id) {
                msg.reactions = message_reactions.clone();
            }
        });

        messages
    }

    pub async fn toggle_message_reaction(
        &self,
        message_id: &str,
        reaction: &str,
        by_username: &str,
    ) -> HashMap<String, Vec<String>> {
        let normalized_by = by_username.trim().to_lowercase();
        let normalized_message_id = message_id.trim().to_string();
        let normalized_reaction = reaction.trim().to_string();

        if normalized_by.is_empty() || normalized_message_id.is_empty() || normalized_reaction.is_empty() {
            return HashMap::new();
        }

        let next_reactions = {
            let mut reactions = self.message_reactions.write().await;
            let message_entry = reactions.entry(normalized_message_id.clone()).or_default();
            let users_entry = message_entry.entry(normalized_reaction.clone()).or_default();

            if let Some(index) = users_entry
                .iter()
                .position(|username| username == &normalized_by)
            {
                users_entry.remove(index);
            } else {
                users_entry.push(normalized_by.clone());
            }

            if users_entry.is_empty() {
                message_entry.remove(&normalized_reaction);
            }

            message_entry.clone()
        };

        let mut mailboxes = self.mailboxes.write().await;
        mailboxes.values_mut().for_each(|messages| {
            messages.iter_mut().for_each(|msg| {
                if msg.id == normalized_message_id {
                    msg.reactions = next_reactions.clone();
                }
            });
        });

        next_reactions
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
