use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub sensor: SensorConfig,
    pub camera: CameraConfig,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:3000".parse().unwrap(),
            data_dir: "data".into(),
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
