use common::{init_config, init_tracing, load_config};
use database::{DatabaseSettings, migrate_or_rescue};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cfg = load_config()?;
    config::init(
        cfg.paths
            .excel_data
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("excel data path is not valid UTF-8"))?,
    )?;
    let db = migrate_or_rescue(&DatabaseSettings {
        db_name: cfg.database.path.to_string_lossy().to_string(),
    })
    .await?;
    init_config(cfg);

    muipserver::run(muipserver::MuipOptions::from_config(db)).await
}
