pub mod adapters;
pub mod api;
pub mod auth;
pub mod automation;
pub mod config;
pub mod database;
pub mod import;
pub mod models;
pub mod store;
pub mod strains;

use adapters::{Camera, Sensor, Switch};
use config::Config;
use database::Database;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct App {
    pub pool: Database,
    pub config: Config,
    pub sensor: Option<Arc<dyn Sensor>>,
    pub camera: Option<Arc<dyn Camera>>,
    pub simulated_switch: Arc<dyn Switch>,
    pub shelly_switch: Arc<dyn Switch>,
    pub capture_lock: Mutex<()>,
    pub control_lock: Mutex<()>,
}
impl App {
    pub async fn open(config: Config) -> anyhow::Result<Arc<Self>> {
        tokio::fs::create_dir_all(config.data_dir.join("photos")).await?;
        let pool =
            Database::open(&config.database, &config.data_dir.join("journal.sqlite3")).await?;
        strains::seed_starter_collection(&pool).await?;
        Ok(Arc::new(Self {
            sensor: adapters::sensor(&config.sensor)?,
            camera: adapters::camera(&config.camera)?,
            config,
            pool,
            simulated_switch: Arc::new(adapters::SimulatedSwitch::default()),
            shelly_switch: Arc::new(adapters::ShellySwitch::new()?),
            capture_lock: Mutex::new(()),
            control_lock: Mutex::new(()),
        }))
    }
}
