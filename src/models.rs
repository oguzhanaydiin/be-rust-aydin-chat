use chrono::{DateTime as ChronoDateTime, Utc};
use mongodb::bson::{oid::ObjectId, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct AuthSessionResponse {
    pub valid: bool,
    pub token: Option<String>,
    pub user_id: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmailOtpRecord {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub email: String,
    pub otp: String,
    pub expires_at: BsonDateTime,
    pub created_at: BsonDateTime,
    #[serde(default)]
    pub is_used: bool,
}

#[derive(Debug, Deserialize)]
pub struct SendEmailOtpRequest {
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct ValidateEmailOtpRequest {
    pub email: String,
    pub otp: String,
}

#[derive(Serialize)]
pub struct SendEmailOtpResponse {
    pub message: String,
    pub otp: String,
    pub expires_in_seconds: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateMessageDTO {
    pub from_user_id: String,
    pub to_user_id: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingMessage {
    pub id: String,
    pub from_user_id: String,
    pub to_user_id: String,
    pub text: String,
    pub created_at: ChronoDateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub message: String,
    pub queued_message_id: String,
}

#[derive(Debug, Deserialize)]
pub struct AckMessagesRequest {
    pub user_id: String,
    pub message_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AckMessagesResponse {
    pub removed_count: usize,
}

#[derive(Debug, Serialize)]
pub struct OnlineUsersResponse {
    pub users: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsClientEvent {
    Register { token: String },
    SendMessage {
        to_user_id: String,
        text: String,
        client_message_id: Option<String>,
    },
    Ack { message_ids: Vec<String> },
    GetOnlineUsers,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsServerEvent {
    Registered { user_id: String },
    OnlineUsers { users: Vec<String> },
    Inbox { messages: Vec<PendingMessage> },
    MessageQueued {
        message_id: String,
        client_message_id: Option<String>,
    },
    MessageDelivered {
        message_id: String,
        client_message_id: Option<String>,
    },
    NewMessage { message: PendingMessage },
    AckResult { removed_count: usize },
    Error { message: String },
}