mod domain;

use crate::domain::SnsNotificationType::{Notification, SubscriptionConfirmation};
use crate::domain::{Message, NotificationType, SnsNotification};
use actix_web::web::Bytes;
use actix_web::{middleware, middleware::Logger, web, App, HttpResponse, HttpServer, Responder};
use dotenv::dotenv;
use serde_json::json;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use regex::Regex;
use tokio_postgres::NoTls;


#[derive(Debug, Clone)]
enum DBType {
    Postgres,
    MySQL(MySqlPool),
}

pub struct AppState {
    db_type: DBType,
    db_url: String,
}


async fn build_mysql_pool(database_url: &str) -> Result<MySqlPool, Box<dyn std::error::Error + Send + Sync>> {
    println!("🚀 Connecting to the MySQL database...");

    let pool = match MySqlPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
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

    Ok(pool)
}

async fn build_pg_pool(database_url: &str) -> Result<tokio_postgres::Client, Box<dyn std::error::Error + Send + Sync>> {
    println!("🚀 Connecting to the PG database...");

    let (client, connection) = tokio_postgres::connect(database_url, NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {}", e);
        }
    });

    println!("✅Connection to the database is successful!");

    Ok(client)
}


#[actix_web::main]
async fn main() -> std::io::Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "actix_web=info");
    }
    dotenv().ok();
    env_logger::init();

    // create the pool depending on the db type, db = MYSQL or = POSTGRES
    let db = std::env::var("DB_TYPE").unwrap_or_else(|_| "MYSQL".into());
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");


    let db_type = match db.as_str() {
        "PG" => {
            DBType::Postgres
        }
        "MYSQL" => {
            let pool = build_mysql_pool(&database_url).await.unwrap();
            DBType::MySQL(pool)
        }
        _ => {
             println!("🔥 Unsupported database type: {}", db);
            std::process::exit(1);
        }
    };


    println!("🚀 Server started successfully");

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Compress::default())
            .app_data(web::Data::new(AppState { db_type: db_type.clone(), db_url: database_url.clone() }))
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
    const MESSAGE: &str = "SES Blacklist API is running!";

    HttpResponse::Ok().json(json!({"status": "success","message": MESSAGE}))
}

async fn is_email_blacklisted(
    path: web::Path<(i32, String)>,
    data: web::Data<AppState>,
) -> impl Responder {
    let (domain_id, email) = path.into_inner();

    let found: Result<bool, String> = match &data.db_type {
        DBType::MySQL(pool) => {
            let query_result = sqlx::query(r#"SELECT * FROM blacklist WHERE domain_id = ? AND email = ?"#)
                .bind(domain_id)
                .bind(email)
                .fetch_one(pool)
                .await;

            match query_result {
                Ok(_) => Ok(true),
                Err(err) => match err {
                    sqlx::Error::RowNotFound => Ok(false),
                    _ => Err(format!("🔥 Failed to query the database: {:?}", err))
                },
            }
        }
        DBType::Postgres => {
            let Ok(client) = build_pg_pool(&data.db_url).await else {
                return HttpResponse::InternalServerError().json(json!({
                    "success": false,
                    "error": "Failed to connect to the database"
                }))
            };
            let query_result = client
                .query_opt(
                    r#"SELECT * FROM blacklist WHERE domain_id = $1 AND email = $2"#,
                    &[&domain_id, &email],
                )
                .await;

            match query_result {
                Ok(Some(_)) => Ok(true),
                Ok(None) => Ok(false),
                Err(err) => Err(format!("🔥 Failed to query the database: {:?}", err)),
            }
        }
    };


    match found {
        Ok(blacklisted) => {
            HttpResponse::Ok().json(json!({
                "success": true,
                "data": {
                    "blacklisted": blacklisted
                }
            }))
        }
        Err(err) => {
            HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": err
            }))
        }
    }
}

async fn handle_sns_notification(
    path: web::Path<i32>,
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

// having a str with: \"Desert Rose Florals, LLC\" <desertroseflorals@gmail.com>" extract only the email
fn extract_email_address(input: &str) -> String {

    // if no index of < or >, return the same string
    if input.find("<").is_none() || input.find(">").is_none() {
        return input.to_string();
    }

    let re = Regex::new(r"<(.*)>").unwrap();
    let caps = re.captures(input);

    match caps {
        Some(caps) => caps[1].to_string(),
        None => input.to_string()
    }
}

async fn handle_bounce(msg: Message, domain_id: i32, data: web::Data<AppState>) -> HttpResponse {
    let reason = serde_json::to_string(&msg.clone()).unwrap();

    match msg.bounce {
        None => {
            println!("Received bounce notification without bounce field: {:?}", msg);
            HttpResponse::Ok().body("ok")
        }
        Some(bounce) => {
            let bounces = bounce
                .bounced_recipients
                .iter()
                .map(|r| extract_email_address(r.email_address.as_str()))
                .collect::<Vec<String>>();


            for bounce in &bounces {
                let query_result: Result<(), String> = match &data.db_type {
                    DBType::MySQL(pool) => {

                        let query_result =
                            sqlx::query(r#"INSERT INTO blacklist (domain_id, email, reason) VALUES (?,?,?)"#)
                                .bind(domain_id)
                                .bind(bounce)
                                .bind(&reason)
                                .execute(pool)
                                .await
                                .map_err(|err: sqlx::Error| err.to_string());

                        match query_result {
                            Ok(_) => Ok(()),
                            Err(err) => Err(err),
                        }
                    }
                    DBType::Postgres => {
                        match build_pg_pool(&data.db_url).await  {
                            Ok(pg) => {

                                let query_result =
                                    pg
                                    .execute(
                                        r#"INSERT INTO blacklist (domain_id, email, reason) VALUES ($1,$2,$3)"#,
                                        &[&domain_id, &bounce, &reason],
                                    )
                                    .await
                                    .map_err(|err| err.to_string());


                                match query_result {
                                    Ok(_) => Ok(()),
                                    Err(err) => Err(err.to_string()),
                                }
                            },
                            Err(err) => {
                               Err(err.to_string())
                            }

                        }
                    }
                };


                if let Err(err) = query_result {
                    if err.contains("Duplicate entry") {
                        println!("blacklist entry already exists for: {}", bounce);
                        return HttpResponse::BadRequest().json(
                            json!({"status": "fail","message": format!("blacklist entry already exists for: {}", bounce)}),
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
    }
}

