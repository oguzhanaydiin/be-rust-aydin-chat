use std::collections::HashMap;
use std::time::Duration;

use actix_web::{http::StatusCode, test, web, App};
use chat_api::{app_state::AppState, routes};
use mongodb::{
    options::{ClientOptions, ServerAddress},
    Client,
};
use serde_json::Value;
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
        mailboxes: RwLock::new(HashMap::new()),
        online_users: RwLock::new(HashMap::new()),
    }
}

#[actix_web::test]
async fn get_status_returns_unhealthy_when_mongodb_is_unreachable() {
    let app_state = web::Data::new(test_app_state("health_route_failure"));
    let app = test::init_service(App::new().app_data(app_state).configure(routes::configure)).await;

    let request = test::TestRequest::get().uri("/health").to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = test::read_body(response).await;
    let payload: Value = serde_json::from_slice(&body).expect("body should be valid json");

    assert_eq!(payload["status"], "unhealthy");
    assert!(payload.get("error").is_some());
}