use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub automation_enabled: bool,
    pub database: DatabaseConfig,
    pub sensor: SensorConfig,
    pub camera: CameraConfig,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:3000".parse().unwrap(),
            data_dir: "data".into(),
            automation_enabled: true,
            database: DatabaseConfig::default(),
            sensor: SensorConfig::default(),
            camera: CameraConfig::default(),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SensorConfig {
    pub adapter: String,
    pub path: PathBuf,
}
impl Default for SensorConfig {
    fn default() -> Self {
        Self {
            adapter: "simulated".into(),
            path: "/sys/bus/iio/devices/iio:device0".into(),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraConfig {
    pub adapter: String,
    pub device: String,
    pub format: String,
    pub size: String,
}
impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            adapter: "simulated".into(),
            device: "/dev/video0".into(),
            format: "mjpeg".into(),
            size: "1280x720".into(),
        }
    }
}
impl Config {
    pub fn load() -> anyhow::Result<Self> {
        match std::env::var("PLANT_CONFIG") {
            Ok(path) => Ok(toml::from_str(&std::fs::read_to_string(path)?)?),
            Err(_) => Ok(Self::default()),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    pub backend: String,
    pub server: String,
    pub name: String,
    pub port: u16,
    pub username_env: String,
    pub password_env: String,
    pub ca_certificate: Option<PathBuf>,
    pub migrate: bool,
}
impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            backend: "sqlite".into(),
            server: String::new(),
            name: String::new(),
            port: 1433,
            username_env: "AZURE_SQL_USERNAME".into(),
            password_env: "AZURE_SQL_PASSWORD".into(),
            ca_certificate: None,
            migrate: true,
        }
    }
}
impl DatabaseConfig {
    pub fn server(&self) -> String {
        std::env::var("AZURE_SQL_SERVER").unwrap_or_else(|_| self.server.clone())
    }
    pub fn name(&self) -> String {
        std::env::var("AZURE_SQL_DATABASE").unwrap_or_else(|_| self.name.clone())
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.server().trim().is_empty(),
            "Set database.server or AZURE_SQL_SERVER for Azure SQL"
        );
        anyhow::ensure!(
            !self.name().trim().is_empty(),
            "Set database.name or AZURE_SQL_DATABASE for Azure SQL"
        );
        let server = self.server();
        anyhow::ensure!(
            server == server.trim() && !server.contains(['/', ';', ',', '=']) && !server.starts_with("tcp:"),
            "Azure SQL server must be a hostname, not a URL or connection string; set database.port separately"
        );
        anyhow::ensure!(
            self.name() == self.name().trim(),
            "Azure SQL database name contains surrounding whitespace"
        );
        anyhow::ensure!(self.port > 0, "database.port must be greater than zero");
        for name in [&self.username_env, &self.password_env] {
            anyhow::ensure!(
                std::env::var(name).is_ok_and(|v| !v.is_empty()),
                "Set the {} environment variable for Azure SQL authentication; .env is not loaded automatically (set -a; source .env; set +a)",
                name
            );
        }
        Ok(())
    }
}
