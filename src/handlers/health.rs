use actix_web::{web, HttpResponse, Responder};
use mongodb::bson::doc;
use serde::Serialize;

use crate::app_state::AppState;

#[derive(Serialize)]
struct HealthyResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct UnhealthyResponse {
    status: &'static str,
    error: String,
}

fn health_response(result: mongodb::error::Result<()>) -> HttpResponse {
    match result {
        Ok(()) => HttpResponse::Ok().json(HealthyResponse { status: "healthy" }),
        Err(error) => {
            eprintln!("Health check failed: {error}");
            HttpResponse::InternalServerError().json(UnhealthyResponse {
                status: "unhealthy",
                error: error.to_string(),
            })
        }
    }
}

pub async fn get_status(data: web::Data<AppState>) -> impl Responder {
    health_response(data.db.run_command(doc! { "ping": 1 }, None).await.map(|_| ()))
}

#[cfg(test)]
mod tests {
    use actix_web::{body::to_bytes, http::StatusCode};
    use serde_json::Value;

    use super::health_response;

    #[actix_web::test]
    async fn health_response_returns_healthy_payload_for_success() {
        let response = health_response(Ok(()));

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body()).await.expect("body should be readable");
        let payload: Value = serde_json::from_slice(&body).expect("body should be valid json");

        assert_eq!(payload, serde_json::json!({ "status": "healthy" }));
    }

    #[actix_web::test]
    async fn health_response_returns_unhealthy_payload_for_error() {
        let response = health_response(Err(std::io::Error::other("database unavailable").into()));

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = to_bytes(response.into_body()).await.expect("body should be readable");
        let payload: Value = serde_json::from_slice(&body).expect("body should be valid json");

        assert_eq!(payload["status"], "unhealthy");
        assert!(payload["error"]
            .as_str()
            .expect("error should be a string")
            .contains("database unavailable"));
    }
}
