//! Packet provenance and garden-scoped germination records.
use crate::{
    api::{ApiError, Result},
    auth::Garden,
    database::{self as db, Transaction},
    store, strains, App,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Serialize)]
pub struct Attempt {
    pub id: String,
    pub seed_id: String,
    pub started_on: String,
    pub seeds_sown: i64,
    pub seeds_germinated: Option<i64>,
    pub notes: String,
    pub created_at: i64,
}
impl db::FromRecord for Attempt {
    fn from_record(r: &db::Record) -> anyhow::Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            seed_id: r.get("seed_id")?,
            started_on: r.get("started_on")?,
            seeds_sown: r.get("seeds_sown")?,
            seeds_germinated: r.get("seeds_germinated")?,
            notes: r.get("notes")?,
            created_at: r.get("created_at")?,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptInput {
    pub started_on: String,
    pub seeds_sown: i64,
    pub seeds_germinated: Option<i64>,
    #[serde(default)]
    pub notes: String,
}
pub(crate) fn check_date(value: &str) -> Result<()> {
    if value.len() != 10
        || NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map(|d| d.format("%Y-%m-%d").to_string() != value)
            .unwrap_or(true)
    {
        return Err(ApiError::bad("Enter a valid date in YYYY-MM-DD format"));
    }
    Ok(())
}
async fn packet(tx: &mut Transaction, garden: &str, id: &str) -> Result<()> {
    if db::query_scalar::<i64>("SELECT COUNT(*) FROM seeds WHERE id=? AND garden_id=?")
        .bind(id)
        .bind(garden)
        .fetch_one(tx)
        .await?
        == 0
    {
        return Err(ApiError::missing());
    }
    Ok(())
}
/// The caller holds the garden write lock until its plant mutation commits.
pub(crate) async fn validate_origin(
    tx: &mut Transaction,
    garden: &str,
    seed: Option<&str>,
    attempt: Option<&str>,
) -> Result<()> {
    if let Some(seed) = seed {
        if db::query_scalar::<i64>("SELECT COUNT(*) FROM seeds WHERE id=? AND garden_id=?")
            .bind(seed)
            .bind(garden)
            .fetch_one(&mut *tx)
            .await?
            == 0
        {
            return Err(ApiError::bad("Choose a seed packet in this garden"));
        }
    }
    if let Some(attempt) = attempt {
        if seed.is_none() || db::query_scalar::<i64>("SELECT COUNT(*) FROM germination_attempts WHERE id=? AND seed_id=? AND garden_id=?")
            .bind(attempt).bind(seed).bind(garden).fetch_one(tx).await? == 0 {
            return Err(ApiError::bad("Choose a germination attempt from the selected packet"));
        }
    }
    Ok(())
}
pub async fn list(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
) -> Result<Json<Vec<Attempt>>> {
    Ok(Json(db::query_as("SELECT * FROM germination_attempts WHERE garden_id=? ORDER BY started_on DESC,created_at DESC,id")
        .bind(garden).fetch_all(&app.pool).await?))
}
async fn save(
    app: &App,
    garden: &str,
    seed: &str,
    id: &str,
    input: AttemptInput,
    existing: bool,
) -> Result<()> {
    check_date(&input.started_on)?;
    if !(1..=1_000_000_000).contains(&input.seeds_sown)
        || input
            .seeds_germinated
            .is_some_and(|n| n < 0 || n > input.seeds_sown)
    {
        return Err(ApiError::bad(
            "Seeds sown must be 1–1000000000; germinated seeds must be between zero and seeds sown",
        ));
    }
    if input.notes.len() > 10000 {
        return Err(ApiError::bad("Notes are too long"));
    }
    let mut tx = app.pool.begin().await?;
    strains::lock_garden(&mut tx, garden).await?;
    packet(&mut tx, garden, seed).await?;
    if existing {
        let changed = db::query("UPDATE germination_attempts SET started_on=?,seeds_sown=?,seeds_germinated=?,notes=? WHERE id=? AND seed_id=? AND garden_id=?")
            .bind(&input.started_on).bind(input.seeds_sown).bind(input.seeds_germinated).bind(&input.notes)
            .bind(id).bind(seed).bind(garden).execute(&mut tx).await?;
        if changed.rows_affected() == 0 {
            return Err(ApiError::missing());
        }
    } else {
        db::query("INSERT INTO germination_attempts(id,garden_id,seed_id,started_on,seeds_sown,seeds_germinated,notes,created_at) VALUES(?,?,?,?,?,?,?,?)")
            .bind(id).bind(garden).bind(seed).bind(&input.started_on).bind(input.seeds_sown)
            .bind(input.seeds_germinated).bind(&input.notes).bind(Utc::now().timestamp()).execute(&mut tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn create(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(seed): Path<String>,
    Json(input): Json<AttemptInput>,
) -> Result<(StatusCode, Json<Value>)> {
    let id = store::id();
    save(&app, &garden, &seed, &id, input, false).await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
pub async fn update(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path((seed, id)): Path<(String, String)>,
    Json(input): Json<AttemptInput>,
) -> Result<StatusCode> {
    save(&app, &garden, &seed, &id, input, true).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn delete(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path((seed, id)): Path<(String, String)>,
) -> Result<StatusCode> {
    let mut tx = app.pool.begin().await?;
    strains::lock_garden(&mut tx, &garden).await?;
    if db::query_scalar::<i64>(
        "SELECT COUNT(*) FROM germination_attempts WHERE id=? AND seed_id=? AND garden_id=?",
    )
    .bind(&id)
    .bind(&seed)
    .bind(&*garden)
    .fetch_one(&mut tx)
    .await?
        == 0
    {
        return Err(ApiError::missing());
    }
    if db::query_scalar::<i64>("SELECT COUNT(*) FROM plants WHERE germination_id=? AND garden_id=?")
        .bind(&id)
        .bind(&*garden)
        .fetch_one(&mut tx)
        .await?
        > 0
    {
        return Err(ApiError::bad("Unlink this attempt from its plants before deleting it. Archived plants retain their seed origin."));
    }
    db::query("DELETE FROM germination_attempts WHERE id=? AND seed_id=? AND garden_id=?")
        .bind(id)
        .bind(seed)
        .bind(&*garden)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
