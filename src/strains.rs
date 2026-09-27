//! Garden-scoped strain records, pedigree validation, and the original owner's starter binder.
use crate::{
    api::{text, ApiError, Result},
    auth::Garden,
    database::{self as db, Database, Transaction},
    store, App,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Strain {
    pub id: String,
    pub name: String,
    pub species: String,
    pub breeder: String,
    pub notes: String,
    pub status: String,
    pub parent_one_id: Option<String>,
    pub parent_two_id: Option<String>,
    pub lineage_note: String,
    pub source_url: String,
    pub created_at: i64,
}
impl db::FromRecord for Strain {
    fn from_record(r: &db::Record) -> anyhow::Result<Self> {
        Ok(Self {
            id: r.get("id")?,
            name: r.get("name")?,
            species: r.get("species")?,
            breeder: r.get("breeder")?,
            notes: r.get("notes")?,
            status: r.get("status")?,
            parent_one_id: r.get("parent_one_id")?,
            parent_two_id: r.get("parent_two_id")?,
            lineage_note: r.get("lineage_note")?,
            source_url: r.get("source_url")?,
            created_at: r.get("created_at")?,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StrainInput {
    pub name: String,
    #[serde(default)]
    pub species: String,
    #[serde(default)]
    pub breeder: String,
    #[serde(default)]
    pub notes: String,
    pub status: String,
    pub parent_one_id: Option<String>,
    pub parent_two_id: Option<String>,
    #[serde(default)]
    pub lineage_note: String,
    #[serde(default)]
    pub source_url: String,
}
pub fn name_key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
// A transaction-held garden row lock serializes lineage edits even across service instances.
// On SQLite the first write also acquires the database write lock.
pub async fn lock_garden(tx: &mut Transaction, garden: &str) -> anyhow::Result<()> {
    db::query("UPDATE gardens SET name=name WHERE id=?")
        .bind(garden)
        .execute(tx)
        .await?;
    Ok(())
}
pub async fn link(
    tx: &mut Transaction,
    garden: &str,
    strain: Option<&str>,
    collect: bool,
) -> Result<()> {
    if let Some(id) = strain {
        let exists: i64 =
            db::query_scalar("SELECT COUNT(*) FROM strains WHERE id=? AND garden_id=?")
                .bind(id)
                .bind(garden)
                .fetch_one(&mut *tx)
                .await?;
        if exists == 0 {
            return Err(ApiError::bad(
                "Choose a strain from this garden's collection",
            ));
        }
        if collect {
            db::query("UPDATE strains SET status='collected' WHERE id=? AND garden_id=?")
                .bind(id)
                .bind(garden)
                .execute(tx)
                .await?;
        }
    }
    Ok(())
}
pub async fn list(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
) -> Result<Json<Vec<Strain>>> {
    Ok(Json(
        db::query_as("SELECT * FROM strains WHERE garden_id=? ORDER BY name_key")
            .bind(garden)
            .fetch_all(&app.pool)
            .await?,
    ))
}
fn check(input: &StrainInput) -> Result<()> {
    text(&input.name, "Strain name", 120)?;
    if input.species.len() > 160
        || input.breeder.len() > 160
        || input.notes.len() > 10000
        || input.lineage_note.len() > 2000
        || input.source_url.len() > 1000
    {
        return Err(ApiError::bad("Strain details are too long"));
    }
    if !["unowned", "wanted", "collected"].contains(&input.status.as_str()) {
        return Err(ApiError::bad("Choose unowned, wanted, or collected"));
    }
    if !input.source_url.is_empty() {
        let url = reqwest::Url::parse(&input.source_url)
            .map_err(|_| ApiError::bad("Source must be an HTTP or HTTPS URL"))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(ApiError::bad("Source must be an HTTP or HTTPS URL"));
        }
    }
    Ok(())
}
async fn save(app: &App, garden: &str, id: &str, input: StrainInput, existing: bool) -> Result<()> {
    check(&input)?;
    let mut tx = app.pool.begin().await?;
    lock_garden(&mut tx, garden).await?;
    let strains: Vec<Strain> = db::query_as("SELECT * FROM strains WHERE garden_id=?")
        .bind(garden)
        .fetch_all(&mut tx)
        .await?;
    if existing && !strains.iter().any(|s| s.id == id) {
        return Err(ApiError::missing());
    }
    if !existing && strains.len() >= 1000 {
        return Err(ApiError::bad("This garden already has 1,000 strains"));
    }
    let key = name_key(&input.name);
    if strains
        .iter()
        .any(|s| s.id != id && name_key(&s.name) == key)
    {
        return Err(ApiError::bad(
            "A strain with this name already exists in this garden",
        ));
    }
    let by_id: HashMap<_, _> = strains.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut stack: Vec<&str> = [
        input.parent_one_id.as_deref(),
        input.parent_two_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect();
    let mut seen = HashSet::new();
    while let Some(parent) = stack.pop() {
        if parent == id {
            return Err(ApiError::bad(
                "A strain cannot be its own ancestor. Choose different parents.",
            ));
        }
        if !seen.insert(parent) {
            continue;
        }
        let ancestor = by_id
            .get(parent)
            .ok_or_else(|| ApiError::bad("Choose parent strains from this garden's collection"))?;
        stack.extend(
            [
                ancestor.parent_one_id.as_deref(),
                ancestor.parent_two_id.as_deref(),
            ]
            .into_iter()
            .flatten(),
        );
    }
    // Inventory always counts as collected. Historical unlocks remain until explicitly edited.
    let holdings:i64=db::query_scalar("SELECT (SELECT COUNT(*) FROM plants WHERE strain_id=? AND garden_id=? AND archived=0)+(SELECT COUNT(*) FROM seeds WHERE strain_id=? AND garden_id=? AND quantity>0)")
        .bind(id).bind(garden).bind(id).bind(garden).fetch_one(&mut tx).await?;
    let status = if holdings > 0 {
        "collected"
    } else {
        &input.status
    };
    if existing {
        db::query("UPDATE strains SET name=?,name_key=?,species=?,breeder=?,notes=?,status=?,parent_one_id=?,parent_two_id=?,lineage_note=?,source_url=? WHERE id=? AND garden_id=?")
            .bind(input.name.trim()).bind(&key).bind(&input.species).bind(&input.breeder).bind(&input.notes).bind(status).bind(&input.parent_one_id).bind(&input.parent_two_id).bind(&input.lineage_note).bind(&input.source_url).bind(id).bind(garden).execute(&mut tx).await?;
    } else {
        db::query("INSERT INTO strains(name,name_key,species,breeder,notes,status,parent_one_id,parent_two_id,lineage_note,source_url,id,garden_id,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(input.name.trim()).bind(&key).bind(&input.species).bind(&input.breeder).bind(&input.notes).bind(status).bind(&input.parent_one_id).bind(&input.parent_two_id).bind(&input.lineage_note).bind(&input.source_url).bind(id).bind(garden).bind(Utc::now().timestamp()).execute(&mut tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn create(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Json(input): Json<StrainInput>,
) -> Result<(StatusCode, Json<Value>)> {
    let id = store::id();
    save(&app, &garden, &id, input, false).await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
pub async fn update(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<StrainInput>,
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
    lock_garden(&mut tx, &garden).await?;
    let exists: i64 = db::query_scalar("SELECT COUNT(*) FROM strains WHERE id=? AND garden_id=?")
        .bind(&id)
        .bind(&garden)
        .fetch_one(&mut tx)
        .await?;
    if exists == 0 {
        return Err(ApiError::missing());
    }
    let links:i64=db::query_scalar("SELECT (SELECT COUNT(*) FROM plants WHERE strain_id=?)+(SELECT COUNT(*) FROM seeds WHERE strain_id=?)+(SELECT COUNT(*) FROM strains WHERE parent_one_id=? OR parent_two_id=?)")
        .bind(&id).bind(&id).bind(&id).bind(&id).fetch_one(&mut tx).await?;
    if links > 0 {
        return Err(ApiError::bad("This strain is linked to plants, seeds, or descendants. Remove those links before deleting it."));
    }
    db::query("DELETE FROM strains WHERE id=? AND garden_id=?")
        .bind(&id)
        .bind(&garden)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Starter {
    name: String,
    aliases: Vec<String>,
    parents: Vec<String>,
    source_url: String,
    #[serde(default)]
    breeder: String,
    #[serde(default)]
    lineage_note: String,
}
pub async fn seed_starter_collection(pool: &Database) -> anyhow::Result<()> {
    let garden = crate::auth::LEGACY_GARDEN;
    let mut tx = pool.begin().await?;
    lock_garden(&mut tx, garden).await?;
    let eligible: i64 = db::query_scalar(
        "SELECT COUNT(*) FROM gardens g JOIN users u ON u.id=g.owner_id WHERE g.id=? AND u.email=?",
    )
    .bind(garden)
    .bind("ally.rippley@gmail.com")
    .fetch_one(&mut tx)
    .await?;
    let version: Option<i64> =
        db::query_scalar("SELECT catalog_version FROM strain_catalog_imports WHERE garden_id=?")
            .bind(garden)
            .fetch_optional(&mut tx)
            .await?;
    let version = version.unwrap_or(0);
    if eligible == 0 || version >= 2 {
        tx.commit().await?;
        return Ok(());
    }
    let mut catalog: Vec<Starter> = if version == 0 {
        serde_json::from_str(include_str!("../resources/starter-strains.json"))?
    } else {
        Vec::new()
    };
    catalog.extend(serde_json::from_str::<Vec<Starter>>(include_str!(
        "../resources/expanded-strains.json"
    ))?);
    let existing: Vec<Strain> = db::query_as("SELECT * FROM strains WHERE garden_id=?")
        .bind(garden)
        .fetch_all(&mut tx)
        .await?;
    let mut ids: HashMap<String, String> = existing
        .iter()
        .map(|s| (name_key(&s.name), s.id.clone()))
        .collect();
    let mut inserted = HashSet::new();
    for strain in &catalog {
        let key = name_key(&strain.name);
        if ids.contains_key(&key) {
            continue;
        }
        let id = store::id();
        let note = if !strain.lineage_note.is_empty() {
            strain.lineage_note.as_str()
        } else if strain.parents.is_empty() {
            "Starter catalog record. Parentage has not been recorded here; unknown does not mean no ancestors."
        } else {
            "Reported catalog lineage, not verified for your particular seeds or cut. Edit to match your records."
        };
        db::query("INSERT INTO strains(id,garden_id,name,name_key,species,breeder,status,lineage_note,source_url,created_at) VALUES(?,?,?,?,'Cannabis',?,'unowned',?,?,?)")
            .bind(&id).bind(garden).bind(&strain.name).bind(&key).bind(&strain.breeder).bind(note).bind(&strain.source_url).bind(Utc::now().timestamp()).execute(&mut tx).await?;
        inserted.insert(id.clone());
        ids.insert(key, id);
    }
    for strain in &catalog {
        let id = &ids[&name_key(&strain.name)];
        if inserted.contains(id) {
            let parents: Vec<_> = strain
                .parents
                .iter()
                .filter_map(|p| ids.get(&name_key(p)))
                .collect();
            db::query("UPDATE strains SET parent_one_id=?,parent_two_id=? WHERE id=?")
                .bind(parents.first().copied())
                .bind(parents.get(1).copied())
                .bind(id)
                .execute(&mut tx)
                .await?;
        }
    }
    let mut matches = HashMap::new();
    let mut newly_linked = HashSet::new();
    for strain in &catalog {
        for name in std::iter::once(&strain.name).chain(strain.aliases.iter()) {
            matches.insert(name_key(name), ids[&name_key(&strain.name)].clone());
        }
    }
    // Match full names only; never guess lineage from a substring or overwrite an existing link.
    for (table, field) in [("plants", "species"), ("seeds", "variety")] {
        let rows:Vec<db::Record>=db::query_as(&format!("SELECT id,name,{field} AS variety FROM {table} WHERE garden_id=? AND strain_id IS NULL"))
            .bind(garden).fetch_all(&mut tx).await?;
        for row in rows {
            let record_id: String = row.get("id")?;
            let name: String = row.get("name")?;
            let variety: String = row.get("variety")?;
            let by_name = matches.get(&name_key(&name));
            let by_variety = matches.get(&name_key(&variety));
            if by_name.is_some() && by_variety.is_some() && by_name != by_variety {
                continue;
            }
            if let Some(strain) = by_variety.or(by_name) {
                db::query(&format!(
                    "UPDATE {table} SET strain_id=? WHERE id=? AND garden_id=?"
                ))
                .bind(strain)
                .bind(record_id)
                .bind(garden)
                .execute(&mut tx)
                .await?;
                newly_linked.insert(strain.clone());
            }
        }
    }
    for id in inserted.iter().chain(newly_linked.iter()) {
        db::query("UPDATE strains SET status='collected' WHERE id=? AND garden_id=? AND (EXISTS(SELECT 1 FROM plants p WHERE p.strain_id=strains.id) OR EXISTS(SELECT 1 FROM seeds s WHERE s.strain_id=strains.id AND s.quantity>0))")
            .bind(id).bind(garden).execute(&mut tx).await?;
    }
    if version == 0 {
        db::query("INSERT INTO strain_catalog_imports(garden_id,imported_at,catalog_version) VALUES(?,?,2)")
            .bind(garden).bind(Utc::now().timestamp()).execute(&mut tx).await?;
    } else {
        db::query("UPDATE strain_catalog_imports SET catalog_version=2 WHERE garden_id=?")
            .bind(garden)
            .execute(&mut tx)
            .await?;
    }
    tx.commit().await
}
