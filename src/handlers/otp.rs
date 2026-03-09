use actix_web::{web, HttpResponse, Responder};
use mongodb::bson::{doc, DateTime as BsonDateTime};
use rand::Rng;
use std::time::{Duration as StdDuration, SystemTime};

use crate::app_state::AppState;
use crate::auth::issue_token;
use crate::models::{
    AuthSessionResponse, EmailOtpRecord, SendEmailOtpRequest, SendEmailOtpResponse,
    ValidateEmailOtpRequest,
};

fn generate_6_digit_otp() -> String {
    let mut rng = rand::thread_rng();
    let value: u32 = rng.gen_range(0..=999_999);
    format!("{value:06}")
}

pub async fn send_email_otp(
    data: web::Data<AppState>,
    req: web::Json<SendEmailOtpRequest>,
) -> impl Responder {
    let email = req.email.trim().to_lowercase();
    if email.is_empty() {
        return HttpResponse::BadRequest().body("email cannot be empty");
    }

    let otp = generate_6_digit_otp();
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
            "otp": &otp,
            "expires_at": expires_at,
            "created_at": now,
            "is_used": false,
        }
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
            // TODO: Send OTP to email provider here. Returning code for local/dev testing.
            HttpResponse::Ok().json(SendEmailOtpResponse {
                message: "OTP created and stored".to_string(),
                otp,
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
            })
        }
        Err(e) => return HttpResponse::InternalServerError().json(e.to_string()),
    };

    let is_not_expired = otp_record.expires_at > BsonDateTime::now();
    let is_code_match = otp_record.otp == req.otp;
    let is_valid = !otp_record.is_used && is_not_expired && is_code_match;

    if is_valid {
        let _ = otp_col
            .update_one(filter, doc! { "$set": { "is_used": true } }, None)
            .await;

        let token = match issue_token(&data.jwt_secret, &email) {
            Ok(value) => value,
            Err(e) => return HttpResponse::InternalServerError().json(e.to_string()),
        };

        return HttpResponse::Ok().json(AuthSessionResponse {
            valid: true,
            token: Some(token),
            user_id: Some(email.clone()),
            email: Some(email),
        });
    }

    HttpResponse::Ok().json(AuthSessionResponse {
        valid: false,
        token: None,
        user_id: None,
        email: None,
    })
}
