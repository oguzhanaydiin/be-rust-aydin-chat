use actix_web::{web, HttpResponse, Responder};
use mongodb::bson::{doc, DateTime as BsonDateTime};
use rand::Rng;
use serde::Serialize;
use std::env;
use std::time::{Duration as StdDuration, SystemTime};

use crate::app_state::AppState;
use crate::auth::issue_token;
use crate::models::{
    AuthSessionResponse, EmailOtpRecord, SendEmailOtpRequest, SendEmailOtpResponse,
    ValidateEmailOtpRequest, User,
};
use crate::otp_hash::{hash_otp, otp_hash_matches};

fn generate_6_digit_otp() -> String {
    let mut rng = rand::thread_rng();
    let value: u32 = rng.gen_range(0..=999_999);
    format!("{value:06}")
}

fn should_include_otp_in_response() -> bool {
    match env::var("APP_ENV") {
        Ok(value) => {
            let env_name = value.trim().to_ascii_lowercase();
            matches!(env_name.as_str(), "dev")
        }
        Err(_) => false,
    }
}

#[derive(Serialize)]
struct ResendEmailRequest {
    from: String,
    to: Vec<String>,
    subject: String,
    html: String,
    text: String,
}

async fn send_otp_with_resend(email: &str, otp: &str) -> Result<(), String> {
    let api_key = env::var("RESEND_API_KEY")
        .map_err(|_| "RESEND_API_KEY is missing".to_string())?;
    let from_email = env::var("RESEND_FROM_EMAIL")
        .map_err(|_| "RESEND_FROM_EMAIL is missing".to_string())?;

    let payload = ResendEmailRequest {
        from: from_email,
        to: vec![email.to_string()],
        subject: "Your OTP Code".to_string(),
        html: format!(
            "<p>Your login code is <strong>{otp}</strong>.</p><p>This code expires in 5 minutes.</p>"
        ),
        text: format!("Your login code is {otp}. This code expires in 5 minutes."),
    };

    let response = reqwest::Client::new()
        .post("https://api.resend.com/emails")
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Resend request failed: {e}"))?;

    if response.status().is_success() {
        return Ok(());
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "failed to read response body".to_string());

    Err(format!("Resend API error: status={status}, body={body}"))
}

pub async fn send_email_otp(
    data: web::Data<AppState>,
    req: web::Json<SendEmailOtpRequest>,
) -> impl Responder {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() {
        return HttpResponse::BadRequest().body("email cannot be empty");
    }

    {
        let mut limiter = data.otp_rate_limiter.write().await;
        if limiter.check_and_record_send(&email).is_err() {
            return HttpResponse::TooManyRequests()
                .body("Too many OTP requests. Please try again later.");
        }
    }

    let otp = generate_6_digit_otp();
    let otp_hash = hash_otp(&data.jwt_secret, &email, &otp);
    let ttl_seconds: i64 = 300;
    let now = BsonDateTime::now();
    let expires_at = BsonDateTime::from_system_time(
        SystemTime::now() + StdDuration::from_secs(ttl_seconds as u64),
    );

    let otp_col = data.db.collection::<EmailOtpRecord>("email_otps");

    if let Err(e) = otp_col
        .delete_many(doc! { "expires_at": { "$lte": BsonDateTime::now() } }, None)
        .await
    {
        return HttpResponse::InternalServerError().json(e.to_string());
    }

    let filter = doc! { "email": &email };
    let update = doc! {
        "$set": {
            "email": &email,
            "otp_hash": &otp_hash,
            "expires_at": expires_at,
            "created_at": now,
            "is_used": false,
        },
        "$unset": { "otp": "" }
    };

    match otp_col
        .update_one(
            filter,
            update,
            mongodb::options::UpdateOptions::builder().upsert(true).build(),
        )
        .await
    {
        Ok(_) => {
            if send_otp_with_resend(&email, &otp).await.is_err() {
                return HttpResponse::InternalServerError().body("failed to send OTP email");
            }

            let response_otp = if should_include_otp_in_response() {
                Some(otp)
            } else {
                None
            };

            HttpResponse::Ok().json(SendEmailOtpResponse {
                message: "OTP created, stored, and sent".to_string(),
                otp: response_otp,
                expires_in_seconds: ttl_seconds,
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(e.to_string()),
    }
}

pub async fn validate_email_otp(
    data: web::Data<AppState>,
    req: web::Json<ValidateEmailOtpRequest>,
) -> impl Responder {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() {
        return HttpResponse::BadRequest().body("email cannot be empty");
    }

    {
        let mut limiter = data.otp_rate_limiter.write().await;
        if limiter.check_and_record_validate(&email).is_err() {
            return HttpResponse::TooManyRequests()
                .body("Too many OTP attempts. Please try again later.");
        }
    }

    let otp_col = data.db.collection::<EmailOtpRecord>("email_otps");
    let filter = doc! { "email": &email };

    let otp_record = match otp_col.find_one(filter.clone(), None).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            return HttpResponse::Ok().json(AuthSessionResponse {
                valid: false,
                token: None,
                user_id: None,
                email: None,
                username: None,
            })
        }
        Err(e) => return HttpResponse::InternalServerError().json(e.to_string()),
    };

    let is_not_expired = otp_record.expires_at > BsonDateTime::now();
    let is_code_match =
        otp_hash_matches(&data.jwt_secret, &email, &req.otp, &otp_record.otp_hash);
    let is_valid = !otp_record.is_used && is_not_expired && is_code_match;

    if is_valid {
        let token = match issue_token(&data.jwt_secret, &email, data.jwt_ttl_secs) {
            Ok(value) => value,
            Err(e) => return HttpResponse::InternalServerError().json(e.to_string()),
        };

        // Ensure a stable user record exists even before username setup.
        let users_col = data.db.collection::<User>("users");
        let user_filter = doc! { "email": &email };
        let now = BsonDateTime::now();
        let user_update = doc! {
            "$set": {
                "email": &email,
                "updated_at": now,
            },
            "$setOnInsert": {
                "created_at": now,
            }
        };
        let find_options = mongodb::options::FindOneAndUpdateOptions::builder()
            .upsert(true)
            .return_document(mongodb::options::ReturnDocument::After)
            .build();

        let user = match users_col
            .find_one_and_update(user_filter, user_update, find_options)
            .await
        {
            Ok(Some(user)) => user,
            Ok(None) => {
                return HttpResponse::InternalServerError()
                    .body("failed to create or load user after OTP validation");
            }
            Err(e) => return HttpResponse::InternalServerError().json(e.to_string()),
        };

        let Some(user_id) = user.id.map(|id| id.to_hex()) else {
            return HttpResponse::InternalServerError()
                .body("user record is missing a stable id");
        };
        let username = user.username;

        // Only burn the OTP after token + stable user_id are ready.
        let _ = otp_col
            .update_one(filter, doc! { "$set": { "is_used": true } }, None)
            .await;

        return HttpResponse::Ok().json(AuthSessionResponse {
            valid: true,
            token: Some(token),
            user_id: Some(user_id),
            email: Some(email),
            username,
        });
    }

    HttpResponse::Ok().json(AuthSessionResponse {
        valid: false,
        token: None,
        user_id: None,
        email: None,
        username: None,
    })
}
