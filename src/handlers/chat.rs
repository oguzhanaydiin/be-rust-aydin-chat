use actix_web::{web, HttpResponse, Responder};
use chrono::Utc;

use crate::app_state::AppState;
use crate::models::{
    AckMessagesRequest, AckMessagesResponse, CreateMessageDTO, OnlineUsersResponse,
    PendingMessage, SendMessageResponse, WsServerEvent,
};

fn generate_message_id() -> String {
    mongodb::bson::oid::ObjectId::new().to_hex()
}

pub async fn send_message(
    data: web::Data<AppState>,
    body: web::Json<CreateMessageDTO>,
) -> impl Responder {
    let from_user_id = body.from_user_id.trim();
    let to_user_id = body.to_user_id.trim();
    let text = body.text.trim();

    if from_user_id.is_empty() || to_user_id.is_empty() || text.is_empty() {
        return HttpResponse::BadRequest().body("from_user_id, to_user_id and text cannot be empty");
    }

    let message = PendingMessage {
        id: generate_message_id(),
        from_user_id: from_user_id.to_string(),
        to_user_id: to_user_id.to_string(),
        text: text.to_string(),
        created_at: Utc::now(),
    };

    let queued_message_id = message.id.clone();
    data.queue_message(message.clone()).await;

    // Try realtime delivery if recipient is online; message stays queued until ack.
    if let Ok(payload) = serde_json::to_string(&WsServerEvent::NewMessage { message }) {
        let _ = data.dispatch_to_user(to_user_id, &payload).await;
    }

    HttpResponse::Ok().json(SendMessageResponse {
        message: "Message added to recipient queue".to_string(),
        queued_message_id,
    })
}

pub async fn get_inbox(
    data: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    let user_id = path.into_inner();
    let normalized_user_id = user_id.trim();

    if normalized_user_id.is_empty() {
        return HttpResponse::BadRequest().body("user_id cannot be empty");
    }

    let pending = data.get_inbox(normalized_user_id).await;

    HttpResponse::Ok().json(pending)
}

pub async fn ack_messages(
    data: web::Data<AppState>,
    body: web::Json<AckMessagesRequest>,
) -> impl Responder {
    let user_id = body.user_id.trim();
    if user_id.is_empty() {
        return HttpResponse::BadRequest().body("user_id cannot be empty");
    }

    if body.message_ids.is_empty() {
        return HttpResponse::Ok().json(AckMessagesResponse { removed_count: 0 });
    }

    let removed_count = data.ack_messages(user_id, &body.message_ids).await;

    HttpResponse::Ok().json(AckMessagesResponse { removed_count })
}

pub async fn get_online_users(data: web::Data<AppState>) -> impl Responder {
    let users = data.online_user_ids().await;
    HttpResponse::Ok().json(OnlineUsersResponse { users })
}
