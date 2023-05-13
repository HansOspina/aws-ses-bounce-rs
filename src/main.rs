mod domain;

use actix_web::{web, middleware::Logger, App, HttpResponse, HttpServer, Responder};
use actix_web::web::Bytes;
use crate::domain::{Blacklist, Message, NotificationType, SnsNotification, SnsNotificationType};
use crate::domain::SnsNotificationType::{Notification, SubscriptionConfirmation};
use dotenv::dotenv;
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
            .app_data(web::Data::new(AppState { db: pool.clone() }))
            .wrap(Logger::new("%a %{User-Agent}i"))
            .service(
                web::resource("/api/{domain_id}/sns-endpoint")
                    .route(web::post().to(handle_sns_notification))
            )
    })
        .bind("127.0.0.1:8000")?
        .run()
        .await
}


async fn handle_sns_notification(path: web::Path<u32>, bytes: Bytes, data: web::Data<AppState>) -> impl Responder {
    let (domain_id) = path.into_inner();

    let Some(notification): Option<SnsNotification> = serde_json::from_slice(&bytes).ok() else {
        println!("Received SNS notification error with bytes: {:?}", bytes);
        return HttpResponse::Ok().body("ok");
    };

    println!("Received SNS notification: {:?}", notification);

    match notification.type_field {
        SubscriptionConfirmation => {
            // To confirm the subscription, visit the SubscribeURL from the incoming message
            println!("Confirm the subscription by visiting: {}", notification.subscribe_url.unwrap());
            HttpResponse::Ok().body("ok")
        }
        Notification => {
            let message = notification.message.unwrap();
            let message: Message = serde_json::from_str(&message).unwrap();

            match message.notification_type {
                NotificationType::Bounce => handle_bounce(message, domain_id, data).await,
                _ => {
                    println!("Received unknown notification type: {:?}", message.notification_type);
                    HttpResponse::Ok().body("ok")
                }
            }
        }
    }
}

async fn handle_bounce(msg: Message, domain_id: u32, data: web::Data<AppState>) -> HttpResponse {
    let bounces = msg.bounce.bounced_recipients.iter().map(|r| r.email_address.as_str()).collect::<Vec<&str>>();

    let reason = serde_json::to_string(&msg).unwrap();

    for bounce in &bounces {


        let query_result = sqlx::query(r#"INSERT INTO blacklist (domain_id, email, reason) VALUES (?,?,?)"#)
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
                    serde_json::json!({"status": "fail","message": "Note with that title already exists"}),
                );
            }

            println!("Failed to execute query: {:?}", err);

            return HttpResponse::InternalServerError()
                .json(serde_json::json!({"status": "error","message": format!("{:?}", err)}));
        }
    }

    println!("Got bounce notification: {:?} for domain: {}", bounces, domain_id);
    HttpResponse::Ok().json(serde_json::json!({"status": "success"}))
}