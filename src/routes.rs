use actix_web::web;

use crate::handlers::{friends, health, otp, users, ws};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health::get_status))
    .route("/users", web::get().to(users::list_users))
    .route("/users/username", web::put().to(users::save_username))
    .route("/friends", web::get().to(friends::list_friends))
    .route("/friends/requests", web::post().to(friends::send_friend_request))
    .route("/friends/requests/accept", web::post().to(friends::accept_friend_request))
    .route("/ws", web::get().to(ws::ws_index))
    .route("/otp/send", web::post().to(otp::send_email_otp))
    .route("/otp/validate", web::post().to(otp::validate_email_otp));
}

