use actix_web::web;

use crate::handlers::{friends, groups, health, otp, users, ws};

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/health", web::get().to(health::get_status))
    .route("/users", web::get().to(users::list_users))
    .route("/users/me", web::get().to(users::get_my_profile))
    .route("/users/username", web::put().to(users::save_username))
    .route("/users/profile", web::put().to(users::update_profile))
    .route("/users/{username}/profile", web::get().to(users::get_user_profile))
    .route("/friends", web::get().to(friends::list_friends))
    .route("/friends/{username}", web::delete().to(friends::remove_friend))
    .route("/friends/requests", web::post().to(friends::send_friend_request))
    .route("/friends/requests/accept", web::post().to(friends::accept_friend_request))
    .route("/groups", web::get().to(groups::list_groups))
    .route("/groups", web::post().to(groups::create_group))
    .route("/groups/{group_id}", web::get().to(groups::get_group_detail))
    .route("/groups/{group_id}/members", web::post().to(groups::add_group_member))
    .route("/groups/{group_id}/members/{username}", web::patch().to(groups::update_group_member_permissions))
    .route("/groups/{group_id}/members/{username}", web::delete().to(groups::remove_group_member))
    .route("/ws", web::get().to(ws::ws_index))
    .route("/otp/send", web::post().to(otp::send_email_otp))
    .route("/otp/validate", web::post().to(otp::validate_email_otp));
}

