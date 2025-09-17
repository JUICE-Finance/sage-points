use actix_cors::Cors;
use actix_web::{get, web, App, HttpResponse, HttpServer, Result};
use serde::{Deserialize, Serialize};

use crate::db::{Database, LeaderboardEntry, UserEvent, UserPoints};

// Request/response structures
#[derive(Debug, Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LeaderboardQuery {
    limit: Option<i64>,
}

impl<T> ApiResponse<T> {
    fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn error(error: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error),
        }
    }
}

// Get user points endpoint
#[get("/api/points/{address}")]
async fn get_user_points(
    address: web::Path<String>,
    db: web::Data<Database>,
) -> Result<HttpResponse> {
    let address = address.into_inner();
    
    // Basic validation - check if it looks like an Ethereum address
    if !address.starts_with("0x") || address.len() != 42 {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<UserPoints>::error(
            "Invalid address format".to_string()
        )));
    }

    match db.get_user_points(&address).await {
        Ok(points) => Ok(HttpResponse::Ok().json(ApiResponse::success(points))),
        Err(e) => {
            eprintln!("Error getting user points: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::<UserPoints>::error(
                "Failed to fetch user points".to_string()
            )))
        }
    }
}

// Get user events endpoint
#[get("/api/events/{address}")]
async fn get_user_events(
    address: web::Path<String>,
    db: web::Data<Database>,
) -> Result<HttpResponse> {
    let address = address.into_inner();
    
    // Basic validation
    if !address.starts_with("0x") || address.len() != 42 {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<Vec<UserEvent>>::error(
            "Invalid address format".to_string()
        )));
    }

    match db.get_user_events(&address).await {
        Ok(events) => Ok(HttpResponse::Ok().json(ApiResponse::success(events))),
        Err(e) => {
            eprintln!("Error getting user events: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::<Vec<UserEvent>>::error(
                "Failed to fetch user events".to_string()
            )))
        }
    }
}

// Get leaderboard endpoint
#[get("/api/leaderboard")]
async fn get_leaderboard(
    query: web::Query<LeaderboardQuery>,
    db: web::Data<Database>,
) -> Result<HttpResponse> {
    let limit = query.limit.unwrap_or(10).min(100); // Default 10, max 100
    
    match db.get_leaderboard(limit).await {
        Ok(leaderboard) => Ok(HttpResponse::Ok().json(ApiResponse::success(leaderboard))),
        Err(e) => {
            eprintln!("Error getting leaderboard: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::<Vec<LeaderboardEntry>>::error(
                "Failed to fetch leaderboard".to_string()
            )))
        }
    }
}

// Health check endpoint
#[get("/health")]
async fn health() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(serde_json::json!({
        "status": "healthy",
        "service": "points-calculator"
    })))
}

// Configure and start the API server
pub async fn run_api_server(db: Database, port: u16) -> std::io::Result<()> {
    println!("🌐 API server running on http://localhost:{}", port);
    
    HttpServer::new(move || {
        // Configure CORS
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .app_data(web::Data::new(db.clone()))
            .service(health)
            .service(get_user_points)
            .service(get_user_events)
            .service(get_leaderboard)
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
