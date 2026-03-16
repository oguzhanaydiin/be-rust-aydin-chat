use actix_web::web;

use crate::handlers::{health, otp, users, ws};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health::get_status))
    .route("/users/username", web::put().to(users::save_username))
    .route("/ws", web::get().to(ws::ws_index))
    .route("/otp/send", web::post().to(otp::send_email_otp))
    .route("/otp/validate", web::post().to(otp::validate_email_otp));
}

