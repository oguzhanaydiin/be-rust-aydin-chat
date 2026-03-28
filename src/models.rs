use chrono::{DateTime as ChronoDateTime, Utc};
use mongodb::bson::{oid::ObjectId, DateTime as BsonDateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize)]
pub struct AuthSessionResponse {
    pub valid: bool,
    pub token: Option<String>,
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub otp: Option<String>,
    pub expires_in_seconds: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_data_url: Option<String>,
    pub created_at: BsonDateTime,
    pub updated_at: BsonDateTime,
}

#[derive(Debug, Deserialize)]
pub struct SaveUsernameRequest {
    pub username: String,
}

#[derive(Serialize)]
pub struct SaveUsernameResponse {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProfileRequest {
    pub avatar_data_url: Option<String>,
}

#[derive(Serialize)]
pub struct UserProfileResponse {
    pub username: String,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_data_url: Option<String>,
}

#[derive(Serialize)]
pub struct PublicProfileResponse {
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_data_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FriendshipStatus {
    Pending,
    Accepted,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Friendship {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub user_a: String,
    pub user_b: String,
    pub requested_by: String,
    pub status: FriendshipStatus,
    pub created_at: BsonDateTime,
    pub updated_at: BsonDateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_at: Option<BsonDateTime>,
}

#[derive(Debug, Deserialize)]
pub struct SendFriendRequestBody {
    pub to_username: String,
}

#[derive(Debug, Deserialize)]
pub struct AcceptFriendRequestBody {
    pub from_username: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FriendSnapshot {
    pub accepted_friends: Vec<String>,
    pub incoming_requests: Vec<String>,
    pub outgoing_requests: Vec<String>,
}



#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingMessage {
    pub id: String,
    pub from_username: String,
    pub to_username: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_data_url: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub reactions: HashMap<String, Vec<String>>,
    pub created_at: ChronoDateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroupPendingMessage {
    pub id: String,
    pub group_id: String,
    pub from_username: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_data_url: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub reactions: HashMap<String, Vec<String>>,
    pub created_at: ChronoDateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GroupRole {
    Leader,
    Member,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatGroup {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub name: String,
    pub created_by: String,
    pub created_at: BsonDateTime,
    pub updated_at: BsonDateTime,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroupMember {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub group_id: String,
    pub username: String,
    pub role: GroupRole,
    #[serde(default)]
    pub can_invite: bool,
    pub added_by: String,
    pub created_at: BsonDateTime,
    pub updated_at: BsonDateTime,
}

#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    #[serde(default)]
    pub member_usernames: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GroupSummaryResponse {
    pub group_id: String,
    pub name: String,
    pub role: GroupRole,
    pub can_invite: bool,
    pub member_count: usize,
}

#[derive(Debug, Serialize)]
pub struct GroupMemberResponse {
    pub username: String,
    pub role: GroupRole,
    pub can_invite: bool,
}

#[derive(Debug, Serialize)]
pub struct GroupDetailResponse {
    pub group_id: String,
    pub name: String,
    pub created_by: String,
    pub members: Vec<GroupMemberResponse>,
}

#[derive(Debug, Deserialize)]
pub struct AddGroupMemberRequest {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateGroupMemberPermissionsRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<GroupRole>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_invite: Option<bool>,
}



#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsClientEvent {
    Register { token: String },
    SendMessage {
        #[serde(alias = "to_user_id")]
        to_username: String,
        text: String,
        image_data_url: Option<String>,
        client_message_id: Option<String>,
    },
    ReactMessage {
        message_id: String,
        #[serde(alias = "to_user_id")]
        to_username: String,
        reaction: String,
    },
    SendGroupMessage {
        group_id: String,
        text: String,
        image_data_url: Option<String>,
        client_message_id: Option<String>,
    },
    ReactGroupMessage {
        message_id: String,
        group_id: String,
        reaction: String,
    },
    Ack { message_ids: Vec<String> },
    AckGroup { message_ids: Vec<String> },
    GetOnlineUsers,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsServerEvent {
    Registered { username: String },
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
    GroupInbox { messages: Vec<GroupPendingMessage> },
    GroupMessageQueued {
        message_id: String,
        group_id: String,
        client_message_id: Option<String>,
    },
    GroupMessageDelivered {
        message_id: String,
        group_id: String,
        client_message_id: Option<String>,
    },
    NewGroupMessage { message: GroupPendingMessage },
    MessageReactionsUpdated {
        message_id: String,
        reactions: HashMap<String, Vec<String>>,
    },
    GroupMessageReactionsUpdated {
        message_id: String,
        group_id: String,
        reactions: HashMap<String, Vec<String>>,
    },
    AckResult { removed_count: usize },
    AckGroupResult { removed_count: usize },
    Error {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
}