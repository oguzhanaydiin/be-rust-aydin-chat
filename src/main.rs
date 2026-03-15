use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use chat_api::{app_state::AppState, db::MongoRepo, routes};
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
    let app_state = web::Data::new(AppState {
        db: db_instance,
        jwt_secret,
        mailboxes: RwLock::new(HashMap::new()),
        online_users: RwLock::new(HashMap::new()),
    });
    
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