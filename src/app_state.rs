use std::collections::HashMap;

use tokio::sync::{mpsc::UnboundedSender, RwLock};

use crate::models::{GroupPendingMessage, PendingMessage};
use crate::otp_limit::OtpRateLimiter;
use crate::pending_dms;

#[derive(Clone)]
pub struct UserConnection {
    pub connection_id: String,
    pub tx: UnboundedSender<String>,
}

pub struct AppState {
    pub db: mongodb::Database,
    pub jwt_secret: String,
    pub jwt_ttl_secs: u64,
    /// When true, DM offline mailbox is write-through to Mongo (`pending_dms`).
    pub persist_dms: bool,
    pub otp_rate_limiter: RwLock<OtpRateLimiter>,
    pub mailboxes: RwLock<HashMap<String, Vec<PendingMessage>>>,
    pub group_mailboxes: RwLock<HashMap<String, Vec<GroupPendingMessage>>>,
    pub message_reactions: RwLock<HashMap<String, HashMap<String, Vec<String>>>>,
    pub group_message_reactions: RwLock<HashMap<String, HashMap<String, Vec<String>>>>,
    pub group_message_members: RwLock<HashMap<String, Vec<String>>>,
    pub online_users: RwLock<HashMap<String, Vec<UserConnection>>>,
}

impl AppState {
    /// Load durable pending DMs into the in-memory mailbox (no-op when `persist_dms` is false).
    pub async fn hydrate_pending_dms(&self) -> Result<usize, String> {
        if !self.persist_dms {
            return Ok(0);
        }

        let messages = pending_dms::load_all_pending(&self.db).await?;
        let count = messages.len();

        let mut mailboxes = self.mailboxes.write().await;
        let mut reactions = self.message_reactions.write().await;
        mailboxes.clear();

        for message in messages {
            reactions
                .entry(message.id.clone())
                .or_insert_with(|| message.reactions.clone());
            mailboxes
                .entry(message.to_username.clone())
                .or_default()
                .push(message);
        }

        Ok(count)
    }

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

    pub async fn queue_message(&self, message: PendingMessage) -> Result<(), String> {
        if self.persist_dms {
            pending_dms::insert_pending_dm(&self.db, &message).await?;
        }

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

        Ok(())
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
        drop(mailboxes);

        if self.persist_dms {
            if let Err(err) =
                pending_dms::set_pending_dm_reactions(&self.db, &normalized_message_id, &next_reactions)
                    .await
            {
                eprintln!("pending DM reaction persist failed: {err}");
            }
        }

        next_reactions
    }

    pub async fn ack_messages(&self, user_id: &str, message_ids: &[String]) -> usize {
        if message_ids.is_empty() {
            return 0;
        }

        if self.persist_dms {
            if let Err(err) = pending_dms::ack_pending_dms(&self.db, user_id, message_ids).await {
                eprintln!("pending DM ack persist failed: {err}");
                return 0;
            }
        }

        let id_set: std::collections::HashSet<&String> = message_ids.iter().collect();
        let mut mailboxes = self.mailboxes.write().await;
        let removed = if let Some(messages) = mailboxes.get_mut(user_id) {
            let before = messages.len();
            messages.retain(|msg| !id_set.contains(&msg.id));
            let removed = before.saturating_sub(messages.len());

            if messages.is_empty() {
                mailboxes.remove(user_id);
            }

            removed
        } else {
            0
        };
        drop(mailboxes);

        // DM is single-recipient: once acked, reaction sidecars are orphaned.
        let mut reactions = self.message_reactions.write().await;
        for id in message_ids {
            reactions.remove(id);
        }

        removed
    }

    pub async fn queue_group_message(&self, message: GroupPendingMessage, recipients: &[String]) {
        {
            let mut reactions = self.group_message_reactions.write().await;
            reactions
                .entry(message.id.clone())
                .or_insert_with(|| message.reactions.clone());
        }

        {
            let mut members = self.group_message_members.write().await;
            members.insert(message.id.clone(), recipients.to_vec());
        }

        let mut mailboxes = self.group_mailboxes.write().await;
        recipients.iter().for_each(|username| {
            mailboxes
                .entry(username.clone())
                .or_default()
                .push(message.clone());
        });
    }

    pub async fn get_group_inbox(&self, user_id: &str) -> Vec<GroupPendingMessage> {
        let mailboxes = self.group_mailboxes.read().await;
        let mut messages = mailboxes.get(user_id).cloned().unwrap_or_default();
        drop(mailboxes);

        let reactions = self.group_message_reactions.read().await;
        messages.iter_mut().for_each(|msg| {
            if let Some(message_reactions) = reactions.get(&msg.id) {
                msg.reactions = message_reactions.clone();
            }
        });

        messages
    }

    pub async fn ack_group_messages(&self, user_id: &str, message_ids: &[String]) -> usize {
        if message_ids.is_empty() {
            return 0;
        }

        let id_set: std::collections::HashSet<&String> = message_ids.iter().collect();
        let mut mailboxes = self.group_mailboxes.write().await;
        let removed = if let Some(messages) = mailboxes.get_mut(user_id) {
            let before = messages.len();
            messages.retain(|msg| !id_set.contains(&msg.id));
            let removed = before.saturating_sub(messages.len());

            if messages.is_empty() {
                mailboxes.remove(user_id);
            }

            removed
        } else {
            0
        };
        drop(mailboxes);

        // Drop member/reaction maps only when no recipient still has a pending copy.
        let mut members = self.group_message_members.write().await;
        let mut reactions = self.group_message_reactions.write().await;
        for id in message_ids {
            let fully_acked = match members.get_mut(id) {
                Some(recipients) => {
                    recipients.retain(|username| username != user_id);
                    recipients.is_empty()
                }
                None => continue,
            };
            if fully_acked {
                members.remove(id);
                reactions.remove(id);
            }
        }

        removed
    }

    pub async fn toggle_group_message_reaction(
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
            let mut reactions = self.group_message_reactions.write().await;
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

        let mut mailboxes = self.group_mailboxes.write().await;
        mailboxes.values_mut().for_each(|messages| {
            messages.iter_mut().for_each(|msg| {
                if msg.id == normalized_message_id {
                    msg.reactions = next_reactions.clone();
                }
            });
        });

        next_reactions
    }

    pub async fn group_message_recipients(&self, message_id: &str) -> Vec<String> {
        let members = self.group_message_members.read().await;
        members.get(message_id).cloned().unwrap_or_default()
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

    pub async fn dispatch_to_users(&self, user_ids: &[String], payload: &str) -> usize {
        let mut delivered = 0usize;
        for user_id in user_ids {
            delivered += self.dispatch_to_user(user_id, payload).await;
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
