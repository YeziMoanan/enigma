use anyhow::{Context, ensure};
use logic::summon::permanent_pool::{SOURCE_DATA_SHA, eligible_six_stars};
use std::{env, fs, path::PathBuf};

fn main() -> anyhow::Result<()> {
    let mut source_sha = None;
    let mut output = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--source-sha" => source_sha = Some(value),
            "--output" => output = Some(PathBuf::from(value)),
            _ => anyhow::bail!("unknown argument {arg}"),
        }
    }
    let source_sha = source_sha.context("--source-sha is required")?;
    ensure!(
        source_sha == SOURCE_DATA_SHA,
        "source SHA must match the pinned Sonetto data"
    );
    let output = output.context("--output is required")?;

    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("workspace root is unavailable")?
        .to_path_buf();
    let data_dir = workspace.join("data/excel2json");
    config::init(data_dir.to_str().context("data path is not valid UTF-8")?)?;
    let bytes = serde_json::to_vec_pretty(&eligible_six_stars(config::configs::get()))?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, bytes).with_context(|| format!("write {}", output.display()))?;
    Ok(())
}
