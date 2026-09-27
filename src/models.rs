use crate::database::{FromRecord, Record};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
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
#[derive(Serialize)]
pub struct Entry {
    pub id: String,
    pub kind: String,
    pub body: String,
    pub occurred_at: i64,
    pub created_at: i64,
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
#[derive(Serialize)]
pub struct Photo {
    pub seed_ids: Vec<String>,
    pub id: String,
    pub filename: String,
    pub captured_at: i64,
    pub source: String,
    pub plant_ids: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Reading {
    pub recorded_at: i64,
    pub temperature_c: f64,
    pub humidity_percent: f64,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
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
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub device_id: String,
    pub enabled: bool,
    pub start_time: String,
    pub end_time: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
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
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub timezone: String,
    pub photo_enabled: bool,
    pub photo_time: String,
}
#[derive(Serialize, Debug)]
pub struct Event {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub occurred_at: i64,
    pub entity_id: Option<String>,
    pub detail: String,
    pub plant_ids: Vec<String>,
}
#[derive(Serialize)]
pub struct Health {
    pub component: String,
    pub last_success: Option<i64>,
    pub last_error: Option<String>,
    pub checked_at: i64,
}

macro_rules! record {
    ($name:ident {$($field:ident),*} $(; $($default:ident),*)?) => {
        impl FromRecord for $name {
            fn from_record(row:&Record)->anyhow::Result<Self> {
                Ok(Self { $($field:row.get(stringify!($field))?,)* $($($default:Default::default(),)*)? })
            }
        }
    };
}
record!(Plant {
    id,
    name,
    species,
    notes,
    archived,
    created_at
});
record!(Entry {id,kind,body,occurred_at,created_at}; plant_ids);
record!(Photo {id,filename,captured_at,source}; plant_ids, seed_ids);
record!(Reading {
    recorded_at,
    temperature_c,
    humidity_percent
});
record!(Device {
    id,
    name,
    role,
    adapter,
    address,
    channel,
    commanded_on,
    reported_on,
    checked_at,
    last_error
});
record!(Schedule {
    device_id,
    enabled,
    start_time,
    end_time
});
record!(Override {
    device_id,
    on_state,
    expires_at
});
record!(Settings {
    timezone,
    photo_enabled,
    photo_time
});
record!(Event {id,kind,title,occurred_at,entity_id,detail}; plant_ids);
record!(Health {
    component,
    last_success,
    last_error,
    checked_at
});

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Seed {
    pub id: String,
    pub name: String,
    pub variety: String,
    pub quantity: i64,
    pub unit: String,
    pub supplier: String,
    pub purchase_year: Option<i64>,
    pub storage_location: String,
    pub notes: String,
    pub created_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedInput {
    pub name: String,
    #[serde(default)]
    pub variety: String,
    pub quantity: i64,
    pub unit: String,
    #[serde(default)]
    pub supplier: String,
    pub purchase_year: Option<i64>,
    #[serde(default)]
    pub storage_location: String,
    #[serde(default)]
    pub notes: String,
}
record!(Seed {
    id,
    name,
    variety,
    quantity,
    unit,
    supplier,
    purchase_year,
    storage_location,
    notes,
    created_at
});
