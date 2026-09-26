use crate::{
    config::{CameraConfig, SensorConfig},
    models::{Device, Reading},
};
use anyhow::{bail, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

#[async_trait]
pub trait Sensor: Send + Sync {
    async fn read(&self, now: i64) -> anyhow::Result<Reading>;
}
#[async_trait]
pub trait Camera: Send + Sync {
    fn extension(&self) -> &'static str;
    fn source(&self) -> &'static str;
    async fn capture(&self, path: &Path) -> anyhow::Result<()>;
}
#[async_trait]
pub trait Switch: Send + Sync {
    async fn status(&self, device: &Device) -> anyhow::Result<bool>;
    async fn set(&self, device: &Device, on: bool) -> anyhow::Result<()>;
}

pub struct SimulatedSensor;
#[async_trait]
impl Sensor for SimulatedSensor {
    async fn read(&self, now: i64) -> anyhow::Result<Reading> {
        let phase = now as f64 / 1800.;
        Ok(Reading {
            recorded_at: now,
            temperature_c: 23. + phase.sin() * 2.,
            humidity_percent: 56. + phase.cos() * 6.,
        })
    }
}
pub struct IioSensor {
    pub path: PathBuf,
}
#[async_trait]
impl Sensor for IioSensor {
    async fn read(&self, now: i64) -> anyhow::Result<Reading> {
        let temperature = tokio::fs::read_to_string(self.path.join("in_temp_input")).await?;
        let humidity =
            tokio::fs::read_to_string(self.path.join("in_humidityrelative_input")).await?;
        let reading = Reading {
            recorded_at: now,
            temperature_c: temperature.trim().parse::<f64>()? / 1000.,
            humidity_percent: humidity.trim().parse::<f64>()? / 1000.,
        };
        validate_reading(&reading)?;
        Ok(reading)
    }
}
pub fn validate_reading(r: &Reading) -> anyhow::Result<()> {
    if !r.temperature_c.is_finite()
        || !r.humidity_percent.is_finite()
        || !(-40. ..=85.).contains(&r.temperature_c)
        || !(0. ..=100.).contains(&r.humidity_percent)
    {
        bail!("Sensor returned an invalid reading");
    }
    Ok(())
}
pub struct SimulatedCamera;
#[async_trait]
impl Camera for SimulatedCamera {
    fn extension(&self) -> &'static str {
        "svg"
    }
    fn source(&self) -> &'static str {
        "simulated"
    }
    async fn capture(&self, path: &Path) -> anyhow::Result<()> {
        tokio::fs::write(path, include_str!("../static/demo-camera.svg")).await?;
        Ok(())
    }
}
pub struct V4l2Camera {
    pub config: CameraConfig,
}
#[async_trait]
impl Camera for V4l2Camera {
    fn extension(&self) -> &'static str {
        "jpg"
    }
    fn source(&self) -> &'static str {
        "v4l2"
    }
    async fn capture(&self, path: &Path) -> anyhow::Result<()> {
        let output = tokio::time::timeout(
            Duration::from_secs(25),
            tokio::process::Command::new("ffmpeg")
                .kill_on_drop(true)
                .args([
                    "-nostdin",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-f",
                    "v4l2",
                    "-input_format",
                    &self.config.format,
                    "-video_size",
                    &self.config.size,
                    "-i",
                    &self.config.device,
                    "-frames:v",
                    "1",
                    "-f",
                    "image2",
                    "-update",
                    "1",
                    "-y",
                ])
                .arg(path)
                .output(),
        )
        .await
        .context("Camera capture timed out")??;
        if !output.status.success() {
            bail!(
                "Camera capture failed: {}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(400)
                    .collect::<String>()
            );
        }
        if tokio::fs::metadata(path).await?.len() == 0 {
            bail!("Camera produced an empty image");
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct SimulatedSwitch {
    states: Mutex<HashMap<String, bool>>,
}
#[async_trait]
impl Switch for SimulatedSwitch {
    async fn status(&self, d: &Device) -> anyhow::Result<bool> {
        Ok(*self
            .states
            .lock()
            .await
            .entry(d.id.clone())
            .or_insert(false))
    }
    async fn set(&self, d: &Device, on: bool) -> anyhow::Result<()> {
        self.states.lock().await.insert(d.id.clone(), on);
        Ok(())
    }
}
pub struct ShellySwitch {
    client: reqwest::Client,
}
impl ShellySwitch {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(4))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }
    async fn rpc(
        &self,
        d: &Device,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let value: serde_json::Value = self
            .client
            .post(format!("{}/rpc", d.address.trim_end_matches('/')))
            .json(&serde_json::json!({"id":1,"method":method,"params":params}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if value.get("error").is_some() {
            bail!("Shelly rejected the command: {}", value["error"]);
        }
        value
            .get("result")
            .cloned()
            .context("Shelly returned no result")
    }
}
#[async_trait]
impl Switch for ShellySwitch {
    async fn status(&self, d: &Device) -> anyhow::Result<bool> {
        #[derive(Deserialize)]
        struct Status {
            output: bool,
        }
        let result = self
            .rpc(d, "Switch.GetStatus", serde_json::json!({"id": d.channel}))
            .await?;
        Ok(serde_json::from_value::<Status>(result)?.output)
    }
    async fn set(&self, d: &Device, on: bool) -> anyhow::Result<()> {
        self.rpc(
            d,
            "Switch.Set",
            serde_json::json!({"id": d.channel,"on":on}),
        )
        .await?;
        Ok(())
    }
}
pub fn sensor(config: &SensorConfig) -> anyhow::Result<Option<Arc<dyn Sensor>>> {
    Ok(match config.adapter.as_str() {
        "disabled" => None,
        "simulated" => Some(Arc::new(SimulatedSensor)),
        "iio" => Some(Arc::new(IioSensor {
            path: config.path.clone(),
        })),
        _ => bail!("sensor.adapter must be disabled, simulated, or iio"),
    })
}
pub fn camera(config: &CameraConfig) -> anyhow::Result<Option<Arc<dyn Camera>>> {
    Ok(match config.adapter.as_str() {
        "disabled" => None,
        "simulated" => Some(Arc::new(SimulatedCamera)),
        "v4l2" => Some(Arc::new(V4l2Camera {
            config: config.clone(),
        })),
        _ => bail!("camera.adapter must be disabled, simulated, or v4l2"),
    })
}
