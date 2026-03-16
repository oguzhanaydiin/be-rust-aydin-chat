use actix_web::{web, HttpRequest, HttpResponse, Responder};
use mongodb::bson::{doc, DateTime as BsonDateTime};

use crate::app_state::AppState;
use crate::auth::verify_token;
use crate::models::{SaveUsernameRequest, SaveUsernameResponse, User};

pub async fn save_username(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<SaveUsernameRequest>,
) -> impl Responder {
    let auth_header = match req.headers().get("Authorization") {
        Some(header) => header,
        None => return HttpResponse::Unauthorized().body("Missing Authorization header"),
    };

    let auth_str = match auth_header.to_str() {
        Ok(s) => s,
        Err(_) => return HttpResponse::BadRequest().body("Invalid Authorization header"),
    };

    let token = if auth_str.starts_with("Bearer ") {
        &auth_str[7..]
    } else {
        return HttpResponse::BadRequest().body("Invalid Authorization format");
    };

    let claims = match verify_token(&data.jwt_secret, token) {
        Ok(c) => c,
        Err(_) => return HttpResponse::Unauthorized().body("Invalid or expired token"),
    };

    let email = claims.email.trim().to_lowercase();
    let username = body.username.trim();

    if username.is_empty() {
        return HttpResponse::BadRequest().body("username cannot be empty");
    }

    let users_col = data.db.collection::<User>("users");

    // Check if username already exists for a different user
    let existing_user = users_col
        .find_one(doc! { "username": username }, None)
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
            "username": username,
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
            username: username.to_string(),
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
