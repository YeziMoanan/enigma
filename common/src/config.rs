use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const CONFIG_TEMPLATE: &str = include_str!("../Config.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub server: ServerSettings,
    #[serde(default)]
    pub muip: MuipConfig,
    #[serde(default)]
    pub muip_gm: MuipGmConfig,
    pub paths: PathConfig,
    pub database: DatabaseConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettings {
    pub host: String,
    pub dns: String,
    pub http_port: u16,
    pub game_port: u16,
    #[serde(default)]
    pub skip_tutorial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuipConfig {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub allow_unspecified_container_bind: bool,
    #[serde(default)]
    pub token_file: Option<PathBuf>,
    #[serde(default)]
    pub token: String,
    pub gm_host: String,
    pub gm_port: u16,
}

impl Default for MuipConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 21100,
            allow_unspecified_container_bind: false,
            token_file: None,
            token: "1999".to_string(),
            gm_host: "127.0.0.1".to_string(),
            gm_port: 21101,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuipGmConfig {
    pub host: String,
    pub port: u16,
    pub enabled: bool,
}

impl Default for MuipGmConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 21101,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConfig {
    pub excel_data: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub path: PathBuf,
}

impl ServerConfig {
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, CONFIG_TEMPLATE)?;
        }

        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn resolve_paths(&mut self, config_dir: &Path) {
        if self.database.path.is_relative() {
            self.database.path = config_dir.join(&self.database.path);
        }
        if self.paths.excel_data.is_relative() {
            self.paths.excel_data = config_dir.join(&self.paths.excel_data);
        }
        if let Some(token_file) = &mut self.muip.token_file
            && token_file.is_relative()
        {
            *token_file = config_dir.join(&*token_file);
        }
    }

    pub fn validate_paths(&mut self) -> anyhow::Result<()> {
        if !self.paths.excel_data.exists() {
            anyhow::bail!(
                "excel data directory not found: {}",
                self.paths.excel_data.display()
            );
        }

        if let Some(parent) = self.database.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if let Some(token_file) = &self.muip.token_file {
            let token = std::fs::read_to_string(token_file)?;
            self.muip.token = token
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(str::trim)
                .unwrap_or_default()
                .to_owned();
        }
        anyhow::ensure!(!self.muip.token.is_empty(), "MUIP token is empty");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn load_or_create_writes_missing_config() {
        let name = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("enigma-config-{name}"));
        let path = dir.join("config.toml");

        let cfg = ServerConfig::load_or_create(&path).unwrap();

        assert!(path.exists());
        assert_eq!(cfg.server.http_port, 21000);
        assert_eq!(cfg.server.game_port, 23301);
        assert!(!cfg.server.skip_tutorial);
        assert_eq!(cfg.muip.port, 21100);
        assert_eq!(cfg.muip_gm.port, 21101);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn protected_muip_token_file_overrides_inline_token() {
        let name = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("enigma-token-config-{name}"));
        let data = dir.join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(dir.join("muip.token"), "protected-test-token\n").unwrap();
        let mut cfg = ServerConfig {
            server: ServerSettings {
                host: "127.0.0.1".into(),
                dns: "example.test".into(),
                http_port: 21000,
                game_port: 23301,
                skip_tutorial: false,
            },
            muip: MuipConfig {
                token_file: Some(PathBuf::from("muip.token")),
                token: "inline-token-must-not-win".into(),
                ..Default::default()
            },
            muip_gm: MuipGmConfig::default(),
            paths: PathConfig {
                excel_data: data.clone(),
            },
            database: DatabaseConfig {
                path: dir.join("db/sonetto.db"),
            },
        };

        cfg.resolve_paths(&dir);
        cfg.validate_paths().unwrap();

        assert_eq!(cfg.muip.token, "protected-test-token");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
