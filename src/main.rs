#![recursion_limit = "512"]

use std::sync::Arc;

use axum::routing::{delete, get, patch, post};
use axum::Router;
use sqlx::PgPool;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

mod config;
mod db;
mod error;
mod models;
mod routes;
mod schema;
mod services;
mod tasks;

use services::rolelogic::RoleLogicClient;
use services::sync::{ConfigSyncEvent, PlayerSyncEvent};

pub struct AppState {
    pub pool: PgPool,
    pub config: config::AppConfig,
    pub player_sync_tx: mpsc::Sender<PlayerSyncEvent>,
    pub config_sync_tx: mpsc::Sender<ConfigSyncEvent>,
    pub rl_client: RoleLogicClient,
    pub http: reqwest::Client,
    pub verify_html: bytes::Bytes,
    pub admin_html: bytes::Bytes,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "referral_code_role=info,tower_http=info".into()),
        )
        .init();

    let app_config = config::AppConfig::from_env();
    let listen_addr = app_config.listen_addr.clone();

    let pool = db::create_pool(&app_config.database_url).await;
    db::run_migrations(&pool).await;
    tracing::info!("Database connected and migrations applied");

    let (player_sync_tx, player_sync_rx) = mpsc::channel::<PlayerSyncEvent>(4096);
    let (config_sync_tx, config_sync_rx) = mpsc::channel::<ConfigSyncEvent>(256);

    let rl_client = RoleLogicClient::new();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client");

    let verify_html =
        bytes::Bytes::from(routes::verification::render_verify_page(&app_config.base_url));
    let admin_html =
        bytes::Bytes::from(routes::admin_page::render_admin_page(&app_config.base_url));

    let state = Arc::new(AppState {
        pool,
        config: app_config,
        player_sync_tx,
        config_sync_tx,
        rl_client,
        http,
        verify_html,
        admin_html,
    });

    tokio::spawn(tasks::player_sync_worker::run(
        player_sync_rx,
        Arc::clone(&state),
    ));
    tokio::spawn(tasks::config_sync_worker::run(
        config_sync_rx,
        Arc::clone(&state),
    ));
    tokio::spawn(tasks::cleanup_expired(Arc::clone(&state)));
    tokio::spawn(tasks::pending_poller::run(Arc::clone(&state)));
    tokio::spawn(tasks::role_expiry_worker::run(Arc::clone(&state)));

    let app = Router::new()
        .nest(
            "/referral-code-role",
            Router::new()
                // RoleLogic contract
                .route("/register", post(routes::plugin::register))
                .route("/config", get(routes::plugin::get_config))
                .route("/config", post(routes::plugin::post_config))
                .route("/config", delete(routes::plugin::delete_config))
                // Admin
                .route("/admin", get(routes::admin_page::admin_page))
                .route("/admin/api/stats", get(routes::admin::stats))
                .route(
                    "/admin/api/batches",
                    get(routes::admin::list_batches).post(routes::admin::create_batch),
                )
                .route(
                    "/admin/api/batches/{id}",
                    patch(routes::admin::update_batch).delete(routes::admin::revoke_batch),
                )
                .route(
                    "/admin/api/batches/{id}/codes",
                    get(routes::admin::list_codes).post(routes::admin::generate_codes),
                )
                .route(
                    "/admin/api/batches/{id}/redemptions",
                    get(routes::admin::list_redemptions),
                )
                .route("/admin/api/codes/{id}", delete(routes::admin::revoke_code))
                .route("/admin/api/codes/{id}/qr.svg", get(routes::admin::code_qr))
                // User redeem
                .route("/verify", get(routes::verification::verify_page))
                .route("/verify/login", get(routes::verification::login))
                .route("/verify/status", get(routes::verification::status))
                .route("/verify/redeem", post(routes::redeem::redeem_code))
                .route("/verify/refresh", post(routes::verification::refresh))
                .route(
                    "/verify/me/redemptions",
                    get(routes::verification::my_redemptions),
                )
                .route("/verify/logout", post(routes::verification::logout))
                // Health & static
                .route("/health", get(routes::health::health))
                .route("/favicon.ico", get(routes::health::favicon)),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    tracing::info!("Server starting on {listen_addr}");

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .expect("Failed to bind listener");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("Shutdown signal received, draining connections...");
        })
        .await
        .expect("Server error");

    tracing::info!("Server stopped");
}
