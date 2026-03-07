use mongodb::bson::{oid::ObjectId, DateTime};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct UserOtpSecret {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub user_id: String,
    pub secret: String,
}

#[derive(Serialize)]
pub struct OtpResponse {
    pub secret: String,
    pub otp: String,
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    pub secret: String,
    pub otp: String,
}

#[derive(Serialize)]
pub struct VerifyResponse {
    pub valid: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmailOtpRecord {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub email: String,
    pub otp: String,
    pub expires_at: DateTime,
    pub created_at: DateTime,
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

// Manages data coming from and going to MongoDB
#[derive(Debug, Serialize, Deserialize)]
pub struct Conversation {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub members: Vec<ObjectId>,
    pub last_message: Option<LastMessagePreview>, 
    #[serde(rename = "created_at")]
    pub created_at: DateTime,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LastMessagePreview {
    pub text: String,
    pub sender: ObjectId,
    pub created_at: DateTime,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<ObjectId>,
    pub conversation_id: ObjectId,
    pub sender_id: ObjectId,
    pub text: String,
    #[serde(rename = "created_at")]
    pub created_at: DateTime,
    // Read status, editing, etc. can be added here
    #[serde(default)] 
    pub is_edited: bool,
}

// DTO (Data Transfer Object) for incoming message requests
#[derive(Deserialize)]
pub struct CreateMessageDTO {
    pub conversation_id: String, // String to ObjectId conversion in handler
    pub sender_id: String,
    pub text: String,
}