mod models;
mod db;

use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use dotenv::dotenv;
use db::MongoRepo;
use models::{Message, CreateMessageDTO, Conversation, LastMessagePreview};
use mongodb::bson::{doc, oid::ObjectId, DateTime};
use futures::stream::TryStreamExt;
use mongodb::Cursor;
use models::{OtpResponse, VerifyRequest, VerifyResponse};
use models::UserOtpSecret;

#[derive(Deserialize)]
struct SetOtpRequest {
    user_id: String,
}

async fn set_user_otp_secret(data: web::Data<AppState>, req: web::Json<SetOtpRequest>) -> impl Responder {
    let user_id = &req.user_id;
    let secret = Secret::generate_secret();
    let secret_str = secret.to_encoded().to_string();

    let otp_secret = UserOtpSecret {
        id: None,
        user_id: user_id.clone(),
        secret: secret_str.clone(),
    };

    let col = data.db.collection::<UserOtpSecret>("user_otp_secrets");
    match col.insert_one(otp_secret, None).await {
        Ok(_) => HttpResponse::Ok().json(OtpResponse { secret: secret_str, otp: create_totp(&secret_str).unwrap().generate_current().unwrap_or_default() }),
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}


#[derive(Deserialize)]
struct VerifyUserOtpRequest {
    user_id: String,
    otp: String,
}

async fn verify_user_otp(data: web::Data<AppState>, req: web::Json<VerifyUserOtpRequest>) -> impl Responder {
    let col = data.db.collection::<UserOtpSecret>("user_otp_secrets");
    let filter = doc! { "user_id": &req.user_id };
    match col.find_one(filter, None).await {
        Some(Ok(user_secret)) => {
            let totp = match create_totp(&user_secret.secret) {
                Ok(t) => t,
                Err(_) => return HttpResponse::BadRequest().json(VerifyResponse { valid: false }),
            };
            let is_valid = totp.check_current(&req.otp).unwrap_or(false);
            HttpResponse::Ok().json(VerifyResponse { valid: is_valid })
        },
        _ => HttpResponse::BadRequest().body("Kullanıcı için secret bulunamadı"),
    }
}
use totp_rs::{Algorithm, TOTP, Secret};

fn create_totp(secret_str: &str) -> Result<TOTP, String> {
    let secret_bytes = Secret::Encoded(secret_str.to_string())
        .to_bytes()
        .map_err(|_| "Geçersiz Base32 secret".to_string())?;

    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret_bytes,
        None,
        "".to_string(),
    )
    .map_err(|e| e.to_string())
}

// OTP Endpoints
async fn generate_otp() -> impl Responder {
    let secret = Secret::generate_secret();
    let secret_str = secret.to_encoded().to_string();

    match create_totp(&secret_str) {
        Ok(totp) => {
            let code = totp.generate_current().unwrap_or_default();
            HttpResponse::Ok().json(OtpResponse {
                secret: secret_str,
                otp: code,
            })
        },
        Err(_) => HttpResponse::InternalServerError().finish(),
    }
}

async fn verify_otp(req: web::Json<VerifyRequest>) -> impl Responder {
    let totp = match create_totp(&req.secret) {
        Ok(t) => t,
        Err(_) => return HttpResponse::BadRequest().json(VerifyResponse { valid: false }),
    };

    let is_valid = totp.check_current(&req.otp).unwrap_or(false);
    HttpResponse::Ok().json(VerifyResponse { valid: is_valid })
}


struct AppState {
    db: mongodb::Database,
}

// 
async fn send_message(
    data: web::Data<AppState>,
    body: web::Json<CreateMessageDTO>,
) -> impl Responder {
    let messages_col = data.db.collection::<Message>("messages");
    let conversations_col = data.db.collection::<Conversation>("conversations");

    let conversation_oid = ObjectId::parse_str(&body.conversation_id).unwrap();
    let sender_oid = ObjectId::parse_str(&body.sender_id).unwrap();
    let now = DateTime::now();

    let new_message = Message {
        id: None, // Mongo will generate
        conversation_id: conversation_oid,
        sender_id: sender_oid,
        text: body.text.clone(),
        created_at: now,
        is_edited: false,
    };

    // Message
    match messages_col.insert_one(new_message, None).await {
        Ok(_) => {
            // update last message preview
            let last_msg = LastMessagePreview {
                text: body.text.clone(),
                sender: sender_oid,
                created_at: now,
            };
            
             let _ = conversations_col.update_one(
                doc! { "_id": conversation_oid },
                doc! { "$set": { "last_message": mongodb::bson::to_bson(&last_msg).unwrap() } },
                None
            ).await;

            HttpResponse::Ok().json("Mesaj gonderildi")
        },
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

// history endpoint (GET /conversations/{id}/messages)
async fn get_history(
    data: web::Data<AppState>,
    path: web::Path<String>,
) -> impl Responder {
    let conversation_id = path.into_inner();
    let conversation_oid = match ObjectId::parse_str(&conversation_id) {
        Ok(id) => id,
        Err(_) => return HttpResponse::BadRequest().body("Gecersiz ID"),
    };

    let messages_col = data.db.collection::<Message>("messages");

    let filter = doc! { "conversation_id": conversation_oid };
    
    let find_options = mongodb::options::FindOptions::builder()
        .sort(doc! { "created_at": -1 }) 
        .limit(50) // Son 50 mesaj
        .build();

    let mut cursor: Cursor<Message> = match messages_col.find(filter, find_options).await {
        Ok(cursor) => cursor,
        Err(e) => return HttpResponse::InternalServerError().json(e.to_string()),
    };

    let mut messages: Vec<Message> = Vec::new();
    while let Ok(Some(msg)) = cursor.try_next().await {
        messages.push(msg);
    }
    
    messages.reverse(); 

    HttpResponse::Ok().json(messages)
}

// new conversation endpoint (POST /conversations)
async fn create_conversation(
    data: web::Data<AppState>,
    body: web::Json<Vec<String>>, // ids of members (user_ids)
) -> impl Responder {
    let conversations_col = data.db.collection::<mongodb::bson::Document>("conversations");

    let member_oids: Vec<ObjectId> = body.iter()
        .map(|id| ObjectId::parse_str(id).unwrap())
        .collect();

    let new_conv = doc! {
        "members": member_oids,
        "created_at": DateTime::now(),
        "last_message": mongodb::bson::Bson::Null, // No messages yet
    };

    match conversations_col.insert_one(new_conv, None).await {
        Ok(result) => HttpResponse::Ok().json(result.inserted_id), // Returns the created Conversation ID
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let mongo_repo = MongoRepo::init().await;
    let db_instance = mongo_repo.get_db().clone();
    
    println!("Server 8080 portunda baslatiliyor...");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(AppState { db: db_instance.clone() }))
            .route("/messages", web::post().to(send_message))
            .route("/conversations/{id}/messages", web::get().to(get_history))
            .route("/otp/set", web::post().to(set_user_otp_secret))
            .route("/otp/verify-user", web::post().to(verify_user_otp))
            .route("/otp/generate", web::get().to(generate_otp))
            .route("/otp/verify", web::post().to(verify_otp))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}