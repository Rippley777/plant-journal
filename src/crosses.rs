//! Garden-scoped cross plans and atomic conversion into collected strains.
use crate::{
    api::{text, ApiError, Result},
    auth::Garden,
    database::{self as db, Transaction},
    store,
    strains::{self, StrainInput},
    App,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Serialize)]
pub struct CrossPlan {
    pub id: String,
    pub name: String,
    pub species: String,
    pub breeder: String,
    pub notes: String,
    pub parent_one_id: String,
    pub parent_two_id: String,
    pub converted_strain_id: Option<String>,
    pub converted_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}
impl db::FromRecord for CrossPlan {
    fn from_record(r: &db::Record) -> anyhow::Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            name: r.get("name")?,
            species: r.get("species")?,
            breeder: r.get("breeder")?,
            notes: r.get("notes")?,
            parent_one_id: r.get("parent_one_id")?,
            parent_two_id: r.get("parent_two_id")?,
            converted_strain_id: r.get("converted_strain_id")?,
            converted_at: r.get("converted_at")?,
            created_at: r.get("created_at")?,
            updated_at: r.get("updated_at")?,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanInput {
    pub name: String,
    pub parent_one_id: String,
    pub parent_two_id: String,
    #[serde(default)]
    pub species: String,
    #[serde(default)]
    pub breeder: String,
    #[serde(default)]
    pub notes: String,
}
// Optional overrides allow a final name or updated notes at conversion. Omitted
// fields retain the saved plan; parent IDs always come from that locked plan.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvertInput {
    pub name: Option<String>,
    pub species: Option<String>,
    pub breeder: Option<String>,
    pub notes: Option<String>,
}
pub async fn list(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
) -> Result<Json<Vec<CrossPlan>>> {
    Ok(Json(
        db::query_as("SELECT * FROM cross_plans WHERE garden_id=? ORDER BY created_at DESC,id")
            .bind(garden)
            .fetch_all(&app.pool)
            .await?,
    ))
}
async fn find(tx: &mut Transaction, garden: &str, id: &str) -> Result<CrossPlan> {
    db::query_as("SELECT * FROM cross_plans WHERE id=? AND garden_id=?")
        .bind(id)
        .bind(garden)
        .fetch_optional(tx)
        .await?
        .ok_or_else(ApiError::missing)
}
async fn save(app: &App, garden: &str, id: &str, input: PlanInput, existing: bool) -> Result<()> {
    text(&input.name, "Working name", 120)?;
    if input.species.len() > 160 || input.breeder.len() > 160 || input.notes.len() > 10000 {
        return Err(ApiError::bad("Cross plan details are too long"));
    }
    let mut tx = app.pool.begin().await?;
    strains::lock_garden(&mut tx, garden).await?;
    if existing {
        if find(&mut tx, garden, id)
            .await?
            .converted_strain_id
            .is_some()
        {
            return Err(ApiError::bad(
                "This plan has already become a strain. Edit the strain record instead.",
            ));
        }
    } else {
        let count: i64 = db::query_scalar("SELECT COUNT(*) FROM cross_plans WHERE garden_id=?")
            .bind(garden)
            .fetch_one(&mut tx)
            .await?;
        if count >= 1000 {
            return Err(ApiError::bad("This garden already has 1,000 cross plans"));
        }
    }
    for parent in [&input.parent_one_id, &input.parent_two_id] {
        strains::link(&mut tx, garden, Some(parent), false).await?;
    }
    let key = strains::name_key(&input.name);
    let duplicate: i64 = db::query_scalar(
        "SELECT COUNT(*) FROM cross_plans WHERE garden_id=? AND name_key=? AND id<>?",
    )
    .bind(garden)
    .bind(&key)
    .bind(id)
    .fetch_one(&mut tx)
    .await?;
    if duplicate > 0 {
        return Err(ApiError::bad(
            "A cross plan with this name already exists in this garden",
        ));
    }
    let now = Utc::now().timestamp();
    if existing {
        db::query("UPDATE cross_plans SET name=?,name_key=?,species=?,breeder=?,notes=?,parent_one_id=?,parent_two_id=?,updated_at=? WHERE id=? AND garden_id=?")
            .bind(input.name.trim()).bind(key).bind(input.species).bind(input.breeder).bind(input.notes)
            .bind(input.parent_one_id).bind(input.parent_two_id).bind(now).bind(id).bind(garden).execute(&mut tx).await?;
    } else {
        db::query("INSERT INTO cross_plans(id,garden_id,name,name_key,species,breeder,notes,parent_one_id,parent_two_id,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
            .bind(id).bind(garden).bind(input.name.trim()).bind(key).bind(input.species).bind(input.breeder).bind(input.notes)
            .bind(input.parent_one_id).bind(input.parent_two_id).bind(now).bind(now).execute(&mut tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn create(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Json(input): Json<PlanInput>,
) -> Result<(StatusCode, Json<Value>)> {
    let id = store::id();
    save(&app, &garden, &id, input, false).await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
pub async fn update(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<PlanInput>,
) -> Result<Json<Value>> {
    save(&app, &garden, &id, input, true).await?;
    Ok(Json(json!({"id":id})))
}
pub async fn delete(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let mut tx = app.pool.begin().await?;
    strains::lock_garden(&mut tx, &garden).await?;
    find(&mut tx, &garden, &id).await?;
    db::query("DELETE FROM cross_plans WHERE id=? AND garden_id=?")
        .bind(id)
        .bind(garden)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn convert(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<ConvertInput>,
) -> Result<(StatusCode, Json<Value>)> {
    let mut tx = app.pool.begin().await?;
    strains::lock_garden(&mut tx, &garden).await?;
    let plan = find(&mut tx, &garden, &id).await?;
    if let Some(strain_id) = plan.converted_strain_id {
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(json!({"id":strain_id}))));
    }
    let strain_id = store::id();
    strains::save_in_transaction(
        &mut tx,
        &garden,
        &strain_id,
        StrainInput {
            name: input.name.unwrap_or(plan.name),
            species: input.species.unwrap_or(plan.species),
            breeder: input.breeder.unwrap_or(plan.breeder),
            notes: input.notes.unwrap_or(plan.notes),
            status: "collected".into(),
            parent_one_id: Some(plan.parent_one_id),
            parent_two_id: Some(plan.parent_two_id),
            lineage_note:
                "Parentage recorded from your cross plan. Update it to match your records.".into(),
            source_url: String::new(),
        },
        false,
    )
    .await?;
    let now = Utc::now().timestamp();
    db::query("UPDATE cross_plans SET converted_strain_id=?,converted_at=?,updated_at=? WHERE id=? AND garden_id=?")
        .bind(&strain_id).bind(now).bind(now).bind(id).bind(garden).execute(&mut tx).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"id":strain_id}))))
}
