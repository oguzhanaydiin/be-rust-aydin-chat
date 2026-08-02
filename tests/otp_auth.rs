use std::collections::HashMap;
use std::time::Duration;

use actix_web::{http::StatusCode, test, web, App};
use chat_api::{
    app_state::AppState,
    auth::{issue_token, verify_token, DEFAULT_JWT_TTL_SECONDS},
    otp_hash::{hash_otp, otp_hash_matches},
    otp_limit::OtpRateLimiter,
    routes,
};
use mongodb::{
    options::{ClientOptions, ServerAddress},
    Client,
};
use tokio::sync::RwLock;

fn test_app_state(database_name: &str) -> AppState {
    let options = ClientOptions::builder()
        .hosts(vec![ServerAddress::Tcp {
            host: "127.0.0.1".to_string(),
            port: Some(1),
        }])
        .server_selection_timeout(Some(Duration::from_millis(50)))
        .build();
    let client = Client::with_options(options).expect("client should build");

    AppState {
        db: client.database(database_name),
        jwt_secret: "test-secret".to_string(),
        jwt_ttl_secs: DEFAULT_JWT_TTL_SECONDS,
        persist_dms: false,
        otp_rate_limiter: RwLock::new(OtpRateLimiter::new()),
        mailboxes: RwLock::new(HashMap::new()),
        group_mailboxes: RwLock::new(HashMap::new()),
        message_reactions: RwLock::new(HashMap::new()),
        group_message_reactions: RwLock::new(HashMap::new()),
        group_message_members: RwLock::new(HashMap::new()),
        online_users: RwLock::new(HashMap::new()),
    }
}

#[actix_web::test]
async fn otp_send_rate_limit_returns_429_without_needing_mongo() {
    let app_state = web::Data::new(test_app_state("otp_send_rate_limit"));
    let app = test::init_service(App::new().app_data(app_state).configure(routes::configure)).await;

    for _ in 0..OtpRateLimiter::SEND_MAX {
        let request = test::TestRequest::post()
            .uri("/otp/send")
            .set_json(serde_json::json!({ "email": "rate@example.com" }))
            .to_request();
        let response = test::call_service(&app, request).await;
        // Unreachable Mongo → 500 after the rate-limit check passes.
        assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    let limited = test::TestRequest::post()
        .uri("/otp/send")
        .set_json(serde_json::json!({ "email": "rate@example.com" }))
        .to_request();
    let response = test::call_service(&app, limited).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[actix_web::test]
async fn otp_validate_rate_limit_returns_429_without_needing_mongo() {
    let app_state = web::Data::new(test_app_state("otp_validate_rate_limit"));
    let app = test::init_service(App::new().app_data(app_state).configure(routes::configure)).await;

    for _ in 0..OtpRateLimiter::VALIDATE_MAX {
        let request = test::TestRequest::post()
            .uri("/otp/validate")
            .set_json(serde_json::json!({ "email": "rate@example.com", "otp": "000000" }))
            .to_request();
        let response = test::call_service(&app, request).await;
        assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    let limited = test::TestRequest::post()
        .uri("/otp/validate")
        .set_json(serde_json::json!({ "email": "rate@example.com", "otp": "000000" }))
        .to_request();
    let response = test::call_service(&app, limited).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[actix_web::test]
async fn otp_success_issues_jwt_usable_for_username_path() {
    // OTP validate → JWT(email) → PUT /users/username keys off claims.email.
    let secret = "test-secret";
    let email = "alice@example.com";
    let otp = "123456";
    let stored = hash_otp(secret, email, otp);
    assert!(otp_hash_matches(secret, email, otp, &stored));

    let token = issue_token(secret, email, 3600).expect("issue");
    let claims = verify_token(secret, &token).expect("verify");
    assert_eq!(claims.email, email);
    assert_eq!(claims.sub, email);
}
