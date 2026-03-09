mod models;
mod db;
mod app_state;
mod auth;
mod handlers;
mod routes;

use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use dotenv::dotenv;
use db::MongoRepo;
use app_state::AppState;
use std::env;
use std::collections::HashMap;
use tokio::sync::RwLock;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    let mongo_repo = MongoRepo::init().await;
    let db_instance = mongo_repo.get_db().clone();
    let jwt_secret = env::var("JWT_SECRET")
        .unwrap_or_else(|_| "dev_insecure_secret_change_me".to_string());
    
    println!("Starting server on port 8080...");

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::permissive())
            .app_data(web::Data::new(AppState {
                db: db_instance.clone(),
                jwt_secret: jwt_secret.clone(),
                mailboxes: RwLock::new(HashMap::new()),
                online_users: RwLock::new(HashMap::new()),
            }))
            .configure(routes::configure)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}