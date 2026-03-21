use actix_web::{web, HttpRequest, HttpResponse, Responder};
use futures::StreamExt;
use mongodb::bson::{doc, DateTime as BsonDateTime};

use crate::app_state::AppState;
use crate::auth::{verify_token, AuthClaims};
use crate::models::{
    AcceptFriendRequestBody, FriendSnapshot, Friendship, FriendshipStatus, SendFriendRequestBody,
    User,
};

fn verify_request_claims(data: &web::Data<AppState>, req: &HttpRequest) -> Result<AuthClaims, HttpResponse> {
    let auth_header = match req.headers().get("Authorization") {
        Some(header) => header,
        None => return Err(HttpResponse::Unauthorized().body("Missing Authorization header")),
    };

    let auth_str = match auth_header.to_str() {
        Ok(s) => s,
        Err(_) => return Err(HttpResponse::BadRequest().body("Invalid Authorization header")),
    };

    let token = if let Some(value) = auth_str.strip_prefix("Bearer ") {
        value
    } else {
        return Err(HttpResponse::BadRequest().body("Invalid Authorization format"));
    };

    verify_token(&data.jwt_secret, token)
        .map_err(|_| HttpResponse::Unauthorized().body("Invalid or expired token"))
}

fn normalize_identity(value: &str) -> String {
    value.trim().to_lowercase()
}

async fn resolve_username_by_email(
    db: &mongodb::Database,
    email: &str,
) -> Result<String, HttpResponse> {
    let users_col = db.collection::<User>("users");

    let found = users_col
        .find_one(doc! { "email": email }, None)
        .await
        .map_err(|e| {
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        })?;

    let username = found
        .and_then(|u| u.username)
        .unwrap_or_default();

    let normalized = normalize_identity(&username);
    if normalized.is_empty() {
        return Err(HttpResponse::BadRequest().body("username is not configured for this account"));
    }

    Ok(normalized)
}

fn sorted_pair(left: &str, right: &str) -> (String, String) {
    if left <= right {
        (left.to_string(), right.to_string())
    } else {
        (right.to_string(), left.to_string())
    }
}

pub async fn build_friend_snapshot(db: &mongodb::Database, username: &str) -> Result<FriendSnapshot, mongodb::error::Error> {
    let normalized = normalize_identity(username);
    let friendships_col = db.collection::<Friendship>("friendships");

    let filter = doc! {
        "$or": [
            { "user_a": &normalized },
            { "user_b": &normalized },
        ]
    };

    let mut cursor = friendships_col.find(filter, None).await?;

    let mut accepted_friends = Vec::new();
    let mut incoming_requests = Vec::new();
    let mut outgoing_requests = Vec::new();

    while let Some(item) = cursor.next().await {
        let friendship = item?;
        let other_username = if friendship.user_a == normalized {
            friendship.user_b.clone()
        } else {
            friendship.user_a.clone()
        };

        match friendship.status {
            FriendshipStatus::Accepted => accepted_friends.push(other_username),
            FriendshipStatus::Pending => {
                if friendship.requested_by == normalized {
                    outgoing_requests.push(other_username);
                } else {
                    incoming_requests.push(other_username);
                }
            }
        }
    }

    accepted_friends.sort_unstable();
    accepted_friends.dedup();

    incoming_requests.sort_unstable();
    incoming_requests.dedup();

    outgoing_requests.sort_unstable();
    outgoing_requests.dedup();

    Ok(FriendSnapshot {
        accepted_friends,
        incoming_requests,
        outgoing_requests,
    })
}

pub async fn list_friends(
    data: web::Data<AppState>,
    req: HttpRequest,
) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = normalize_identity(&claims.email);
    let username = match resolve_username_by_email(&data.db, &email).await {
        Ok(username) => username,
        Err(response) => return response,
    };

    match build_friend_snapshot(&data.db, &username).await {
        Ok(snapshot) => HttpResponse::Ok().json(snapshot),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Database error",
            "message": e.to_string()
        })),
    }
}

pub async fn send_friend_request(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<SendFriendRequestBody>,
) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = normalize_identity(&claims.email);
    let from_username = match resolve_username_by_email(&data.db, &email).await {
        Ok(username) => username,
        Err(response) => return response,
    };
    let to_username = normalize_identity(&body.to_username);

    if to_username.is_empty() {
        return HttpResponse::BadRequest().body("to_username is required");
    }

    if to_username == from_username {
        return HttpResponse::BadRequest().body("cannot send request to self");
    }

    let users_col = data.db.collection::<User>("users");
    let target_exists = match users_col
        .find_one(doc! { "username": &to_username }, None)
        .await
    {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        }
    };

    if !target_exists {
        return HttpResponse::NotFound().body("target user not found");
    }

    let (user_a, user_b) = sorted_pair(&from_username, &to_username);
    let friendships_col = data.db.collection::<Friendship>("friendships");

    let existing = match friendships_col
        .find_one(doc! { "user_a": &user_a, "user_b": &user_b }, None)
        .await
    {
        Ok(value) => value,
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        }
    };

    if let Some(friendship) = existing {
        if friendship.status == FriendshipStatus::Accepted {
            return HttpResponse::Conflict().body("already friends");
        }

        return HttpResponse::Conflict().body("friend request already exists");
    }

    let now = BsonDateTime::now();
    let doc = Friendship {
        id: None,
        user_a,
        user_b,
        requested_by: from_username.clone(),
        status: FriendshipStatus::Pending,
        created_at: now,
        updated_at: now,
        accepted_at: None,
    };

    if let Err(e) = friendships_col.insert_one(doc, None).await {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Database error",
            "message": e.to_string()
        }));
    }

    HttpResponse::Created().json(serde_json::json!({ "ok": true }))
}

pub async fn accept_friend_request(
    data: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<AcceptFriendRequestBody>,
) -> impl Responder {
    let claims = match verify_request_claims(&data, &req) {
        Ok(claims) => claims,
        Err(response) => return response,
    };

    let email = normalize_identity(&claims.email);
    let current_username = match resolve_username_by_email(&data.db, &email).await {
        Ok(username) => username,
        Err(response) => return response,
    };
    let from_username = normalize_identity(&body.from_username);

    if from_username.is_empty() {
        return HttpResponse::BadRequest().body("from_username is required");
    }

    if current_username == from_username {
        return HttpResponse::BadRequest().body("cannot accept self request");
    }

    let (user_a, user_b) = sorted_pair(&current_username, &from_username);
    let friendships_col = data.db.collection::<Friendship>("friendships");

    let existing = match friendships_col
        .find_one(doc! { "user_a": &user_a, "user_b": &user_b }, None)
        .await
    {
        Ok(value) => value,
        Err(e) => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "Database error",
                "message": e.to_string()
            }))
        }
    };

    let Some(friendship) = existing else {
        return HttpResponse::NotFound().body("friend request not found");
    };

    if friendship.status == FriendshipStatus::Accepted {
        return HttpResponse::Ok().json(serde_json::json!({ "ok": true, "already_accepted": true }));
    }

    if friendship.requested_by != from_username {
        return HttpResponse::BadRequest().body("there is no incoming request from this user");
    }

    let now = BsonDateTime::now();
    if let Err(e) = friendships_col
        .update_one(
            doc! { "user_a": &user_a, "user_b": &user_b, "status": "pending", "requested_by": &from_username },
            doc! { "$set": { "status": "accepted", "updated_at": now, "accepted_at": now } },
            None,
        )
        .await
    {
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": "Database error",
            "message": e.to_string()
        }));
    }

    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}
