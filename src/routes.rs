use actix_web::web;

use crate::handlers::{chat, health, otp, ws};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health::get_status))
    .route("/messages", web::post().to(chat::send_message))
    .route("/messages/inbox/{id}", web::get().to(chat::get_inbox))
    .route("/messages/ack", web::post().to(chat::ack_messages))
    .route("/users/online", web::get().to(chat::get_online_users))
        .route("/ws", web::get().to(ws::ws_index))
        .route("/otp/send", web::post().to(otp::send_email_otp))
        .route("/otp/validate", web::post().to(otp::validate_email_otp));
}

