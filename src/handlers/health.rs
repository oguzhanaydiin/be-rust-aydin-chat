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

pub async fn get_status(data: web::Data<AppState>) -> impl Responder {
    match data.db.run_command(doc! { "ping": 1 }, None).await {
        Ok(_) => HttpResponse::Ok().json(HealthyResponse { status: "healthy" }),
        Err(e) => {
            eprintln!("Health check failed: {e}");
            HttpResponse::InternalServerError().json(UnhealthyResponse {
                status: "unhealthy",
                error: e.to_string(),
            })
        }
    }
}
