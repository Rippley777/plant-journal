use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Clone, Serialize, Deserialize, FromRow, Debug)]
pub struct Plant {
    pub id: String,
    pub name: String,
    pub species: String,
    pub notes: String,
    pub archived: bool,
    pub created_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlantInput {
    pub name: String,
    #[serde(default)]
    pub species: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub archived: bool,
}
#[derive(Serialize, FromRow)]
pub struct Entry {
    pub id: String,
    pub kind: String,
    pub body: String,
    pub occurred_at: i64,
    pub created_at: i64,
    #[sqlx(skip)]
    pub plant_ids: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryInput {
    pub kind: String,
    pub body: String,
    pub occurred_at: i64,
    pub plant_ids: Vec<String>,
}
#[derive(Serialize, FromRow)]
pub struct Photo {
    pub id: String,
    pub filename: String,
    pub captured_at: i64,
    pub source: String,
    #[sqlx(skip)]
    pub plant_ids: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, FromRow, Debug)]
pub struct Reading {
    pub recorded_at: i64,
    pub temperature_c: f64,
    pub humidity_percent: f64,
}
#[derive(Clone, Serialize, Deserialize, FromRow, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub role: String,
    pub adapter: String,
    pub address: String,
    pub channel: i64,
    pub commanded_on: Option<bool>,
    pub reported_on: Option<bool>,
    pub checked_at: Option<i64>,
    pub last_error: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceInput {
    pub name: String,
    pub role: String,
    pub adapter: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub channel: i64,
}
#[derive(Clone, Serialize, Deserialize, FromRow, Debug)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub device_id: String,
    pub enabled: bool,
    pub start_time: String,
    pub end_time: String,
}
#[derive(Clone, Serialize, Deserialize, FromRow, Debug)]
pub struct Override {
    pub device_id: String,
    pub on_state: bool,
    pub expires_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverrideInput {
    pub on: bool,
    #[serde(default = "default_minutes")]
    pub minutes: i64,
}
fn default_minutes() -> i64 {
    60
}
#[derive(Clone, Serialize, Deserialize, FromRow, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub timezone: String,
    pub photo_enabled: bool,
    pub photo_time: String,
}
#[derive(Serialize, FromRow, Debug)]
pub struct Event {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub occurred_at: i64,
    pub entity_id: Option<String>,
    pub detail: String,
    #[sqlx(skip)]
    pub plant_ids: Vec<String>,
}
#[derive(Serialize, FromRow)]
pub struct Health {
    pub component: String,
    pub last_success: Option<i64>,
    pub last_error: Option<String>,
    pub checked_at: i64,
}
