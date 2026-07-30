use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use chat_api::{
    app_state::AppState,
    auth::DEFAULT_JWT_TTL_SECONDS,
    db::MongoRepo,
    routes,
};
use dotenv::dotenv;
use std::env;
use std::io::{Error as IoError, ErrorKind};
use std::collections::HashMap;
use tokio::sync::RwLock;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let mongo_repo = MongoRepo::init().await;
    let db_instance = mongo_repo.get_db().clone();
    let jwt_secret = env::var("JWT_SECRET")
        .map_err(|_| IoError::new(ErrorKind::InvalidInput, "JWT_SECRET is missing"))?;
    let jwt_ttl_secs = env::var("JWT_TTL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_JWT_TTL_SECONDS);
    let app_state = AppState {
        db: db_instance,
        jwt_secret,
        jwt_ttl_secs,
        persist_dms: true,
        otp_rate_limiter: RwLock::new(chat_api::otp_limit::OtpRateLimiter::new()),
        mailboxes: RwLock::new(HashMap::new()),
        group_mailboxes: RwLock::new(HashMap::new()),
        message_reactions: RwLock::new(HashMap::new()),
        group_message_reactions: RwLock::new(HashMap::new()),
        group_message_members: RwLock::new(HashMap::new()),
        online_users: RwLock::new(HashMap::new()),
    };

    match app_state.hydrate_pending_dms().await {
        Ok(count) => println!("Hydrated {count} pending DM(s) from MongoDB."),
        Err(err) => {
            return Err(IoError::new(
                ErrorKind::Other,
                format!("failed to hydrate pending DMs: {err}"),
            ));
        }
    }

    let app_state = web::Data::new(app_state);

    println!("Starting server on port 8080...");

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive())
            .app_data(app_state.clone())
            .configure(routes::configure)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}