use std::collections::HashSet;

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use futures::StreamExt;
use mongodb::bson::{doc, DateTime as BsonDateTime};

use crate::app_state::AppState;
use crate::auth::{verify_token, AuthClaims};
use crate::models::{SaveUsernameRequest, SaveUsernameResponse, User};

fn verify_request_claims(data: &web::Data<AppState>, req: &HttpRequest) -> Result<AuthClaims, HttpResponse> {
    let auth_header = match req.headers().get("Authorization") {
        Some(header) => header,
        None => return Err(HttpResponse::Unauthorized().body("Missing Authorization header")),
    };

    let auth_str = match auth_header.to_str() {
        Ok(s) => s,
        Err(_) => return Err(HttpResponse::BadRequest().body("Invalid Authorization header")),
    };

    let token = if auth_str.starts_with("Bearer ") {
        &auth_str[7..]
    } else {
        return Err(HttpResponse::BadRequest().body("Invalid Authorization format"));
    };

    verify_token(&data.jwt_secret, token)
        .map_err(|_| HttpResponse::Unauthorized().body("Invalid or expired token"))
}

pub async fn list_users(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(response) = verify_request_claims(&data, &req) {
        return response;
    }

    let users_col = data.db.collection::<User>("users");
    let mut cursor = match users_col
        .find(doc! { "username": { "$exists": true, "$ne": "" } }, None)
        .await
    {
        Ok(cursor) => cursor,
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }));
        }
    };

    let mut users = Vec::new();
    let mut seen = HashSet::new();

    while let Some(item) = cursor.next().await {
        match item {
            Ok(user) => {
                if let Some(username) = user.username {
                    let normalized = username.trim().to_lowercase();
                    if !normalized.is_empty() && seen.insert(normalized.clone()) {
                        users.push(normalized);
                    }
                }
            }
            Err(e) => {
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "Database error",
                    "message": e.to_string()
                }));
            }
        }
    }

    users.sort_unstable();
    HttpResponse::Ok().json(serde_json::json!({ "users": users }))
}

pub async fn save_username(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<SaveUsernameRequest>,
) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = claims.email.trim().to_lowercase();
    let username = body.username.trim().to_lowercase();

    if username.is_empty() {
        return HttpResponse::BadRequest().body("username cannot be empty");
    }

    let users_col = data.db.collection::<User>("users");

    // Check if username already exists for a different user
    let existing_user = users_col
        .find_one(doc! { "username": &username }, None)
        .await;

    match existing_user {
        Ok(Some(user)) => {
            // Username exists, check if it belongs to the current user
            if user.email.to_lowercase() != email {
                return HttpResponse::Conflict().json(
                    serde_json::json!({
                        "error": "Username already taken",
                        "message": "This username is already in use by another user"
                    })
                );
            }
        }
        Ok(None) => {
            // Username doesn't exist, proceed
        }
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(serde_json::json!({
                    "error": "Database error",
                    "message": e.to_string()
                }));
        }
    }

    let filter = doc! { "email": &email };
    let now = BsonDateTime::now();

    let update = doc! {
        "$set": {
            "email": &email,
            "username": &username,
            "updated_at": now,
        },
        "$setOnInsert": {
            "created_at": now,
        }
    };

    match users_col
        .update_one(
            filter,
            update,
            mongodb::options::UpdateOptions::builder().upsert(true).build(),
        )
        .await
    {
        Ok(_) => HttpResponse::Ok().json(SaveUsernameResponse {
            username,
        }),
        Err(e) => {
            // Check if it's a duplicate key error (E11000)
            let error_msg = e.to_string();
            if error_msg.contains("E11000") || error_msg.contains("duplicate") {
                HttpResponse::Conflict().json(
                    serde_json::json!({
                        "error": "Username already taken",
                        "message": "This username is already in use by another user"
                    })
                )
            } else {
                HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "Database error",
                    "message": e.to_string()
                }))
            }
        }
    }
}
