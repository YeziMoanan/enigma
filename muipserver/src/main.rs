use common::{init_config, init_tracing, load_config};
use database::db::game::mail_campaign;
use database::{DatabaseSettings, migrate_or_rescue};
use tracing::info;

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
    let (accounts, mails) =
        mail_campaign::reconcile_release_announcements(&db, common::time::ServerTime::now_ms())
            .await?;
    info!(accounts, mails, "Reconciled release announcement mail");
    init_config(cfg);

    muipserver::run(muipserver::MuipOptions::from_config(db)).await
}
