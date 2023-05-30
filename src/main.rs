mod domain;

use crate::domain::SnsNotificationType::{Notification, SubscriptionConfirmation};
use crate::domain::{Message, NotificationType, SnsNotification};
use actix_web::web::Bytes;
use actix_web::{middleware, middleware::Logger, web, App, HttpResponse, HttpServer, Responder};
use dotenv::dotenv;
use serde_json::json;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};

pub struct AppState {
    db: MySqlPool,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "actix_web=info");
    }
    dotenv().ok();
    env_logger::init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = match MySqlPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
    {
        Ok(pool) => {
            println!("✅Connection to the database is successful!");
            pool
        }
        Err(err) => {
            println!("🔥 Failed to connect to the database: {:?}", err);
            std::process::exit(1);
        }
    };

    println!("🚀 Server started successfully");

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Compress::default())
            .app_data(web::Data::new(AppState { db: pool.clone() }))
            .wrap(Logger::new(
                r#"%a "%r" %s %b "%{Referer}i" "%{User-Agent}i" %T"#,
            ))
            .service(
                web::resource("/api/v1/health_check").route(web::get().to(health_checker_handler)),
            )
            .service(
                web::resource("/api/{domain_id}/sns-endpoint")
                    .route(web::post().to(handle_sns_notification)),
            )
            .service(
                web::resource("/api/{domain_id}/is-blacklisted/{email}")
                    .route(web::get().to(is_email_blacklisted)),
            )
    })
    .bind("0.0.0.0:8000")?
    .run()
    .await
}

async fn health_checker_handler() -> impl Responder {
    const MESSAGE: &str = "iBuyFlowers Product Server";

    HttpResponse::Ok().json(json!({"status": "success","message": MESSAGE}))
}

// return json success:true, data: {blacklisted: true/false}
async fn is_email_blacklisted(
    path: web::Path<(u32, String)>,
    data: web::Data<AppState>,
) -> impl Responder {
    let (domain_id, email) = path.into_inner();

    let query_result = sqlx::query(r#"SELECT * FROM blacklist WHERE domain_id = ? AND email = ?"#)
        .bind(domain_id)
        .bind(email)
        .fetch_one(&data.db)
        .await;

    match query_result {
        Ok(_) => HttpResponse::Ok().json(json!({
            "success": true,
            "data": {
                "blacklisted": true
            }
        })),
        Err(err) => match err {
            sqlx::Error::RowNotFound => HttpResponse::Ok().json(json!({
                "success": true,
                "data": {
                    "blacklisted": false
                }
            })),
            _ => {
                println!("🔥 Failed to query blacklist: {:?}", err);
                HttpResponse::Ok().json(json!({
                    "success": false,
                    "error": "Failed to query blacklist"
                }))
            }
        },
    }
}

async fn handle_sns_notification(
    path: web::Path<u32>,
    bytes: Bytes,
    data: web::Data<AppState>,
) -> impl Responder {
    let domain_id = path.into_inner();

    let Some(notification): Option<SnsNotification> = serde_json::from_slice(&bytes).ok() else {
        println!("Received SNS notification error with bytes: {:?}", bytes);
        return HttpResponse::Ok().body("ok");
    };

    println!("Received SNS notification: {:?}", notification);

    match notification.type_field {
        SubscriptionConfirmation => {
            let a = &notification.subscribe_url.unwrap();
            // To confirm the subscription, visit the SubscribeURL from the incoming message
            println!("Confirm the subscription by visiting: {}", a);
            // Subscribe to the topic using reqwest
            let client = reqwest::Client::new();
            let _ = client.get(a).send().await;

            HttpResponse::Ok().body("ok")
        }
        Notification => {
            let message = notification.message.unwrap();
            let message: Message = serde_json::from_str(&message).unwrap();

            match message.notification_type {
                NotificationType::Bounce => handle_bounce(message, domain_id, data).await,
                _ => {
                    println!(
                        "Received unknown notification type: {:?}",
                        message.notification_type
                    );
                    HttpResponse::Ok().body("ok")
                }
            }
        }
    }
}

async fn handle_bounce(msg: Message, domain_id: u32, data: web::Data<AppState>) -> HttpResponse {
    let bounces = msg
        .bounce
        .bounced_recipients
        .iter()
        .map(|r| r.email_address.as_str())
        .collect::<Vec<&str>>();

    let reason = serde_json::to_string(&msg).unwrap();

    for bounce in &bounces {
        let query_result =
            sqlx::query(r#"INSERT INTO blacklist (domain_id, email, reason) VALUES (?,?,?)"#)
                .bind(domain_id)
                .bind(bounce)
                .bind(&reason)
                .execute(&data.db)
                .await
                .map_err(|err: sqlx::Error| err.to_string());

        if let Err(err) = query_result {
            if err.contains("Duplicate entry") {
                println!("Note with that title already exists {:?}", err);
                return HttpResponse::BadRequest().json(
                    json!({"status": "fail","message": "Note with that title already exists"}),
                );
            }

            println!("Failed to execute query: {:?}", err);

            return HttpResponse::InternalServerError()
                .json(json!({"status": "error","message": format!("{:?}", err)}));
        }
    }

    println!(
        "Got bounce notification: {:?} for domain: {}",
        bounces, domain_id
    );
    HttpResponse::Ok().json(json!({"status": "success"}))
}
