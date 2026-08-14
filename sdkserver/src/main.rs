use common::{host, http_port, init_config, init_tracing, load_config};
use database::{DatabaseSettings, SqlitePool, migrate_or_rescue};
use reqwest::Client;
use std::net::SocketAddr;
use tracing::info;

mod access_control;
mod handlers;
mod middleware;
mod models;

use middleware::crypto::sdk_encryption;
use middleware::logging::full_logger;

#[derive(Clone)]
pub struct SdkState {
    pub http_client: Client,
}

#[derive(Clone)]
pub struct AppState {
    pub sdk: SdkState,
    pub db: SqlitePool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cfg = load_config()?;
    init_config(cfg.clone());

    let db = migrate_or_rescue(&DatabaseSettings {
        db_name: cfg.database.path.to_string_lossy().to_string(),
    })
    .await?;

    let state = AppState {
        sdk: SdkState {
            http_client: Client::new(),
        },
        db,
    };

    let with_encryption = handlers::router::account_router()
        .merge(handlers::router::trade_router())
        .layer(axum::middleware::from_fn(full_logger))
        .layer(axum::middleware::from_fn(sdk_encryption));

    let without_encryption = handlers::router::game_router()
        .merge(handlers::router::jsp_router())
        .merge(handlers::router::index_router())
        .layer(axum::middleware::from_fn(full_logger));

    let app = with_encryption.merge(without_encryption).with_state(state);
    let addr: SocketAddr = format!("{}:{}", host(), http_port()).parse()?;
    info!("SDK is listening on http://{}", addr);

    axum_server::bind(addr)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;
    Ok(())
}
