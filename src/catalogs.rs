//! Optional starter collections. Imports are explicit, atomic, and once per garden.
use crate::{
    api::{ApiError, Result},
    auth::Garden,
    database::{self as db, Transaction},
    strains::{self, Starter},
    App,
};
use axum::{extract::State, Extension, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};

#[derive(Deserialize)]
struct Catalog {
    id: String,
    name: String,
    description: String,
    entries: Vec<Starter>,
}
#[derive(Serialize)]
pub struct CatalogInfo {
    id: String,
    name: String,
    description: String,
    count: usize,
    examples: Vec<String>,
}
fn catalogs() -> anyhow::Result<Vec<Catalog>> {
    let mut packs: Vec<Catalog> =
        serde_json::from_str(include_str!("../resources/garden-catalogs.json"))?;
    let mut cannabis: Vec<Starter> =
        serde_json::from_str(include_str!("../resources/starter-strains.json"))?;
    cannabis.extend(serde_json::from_str::<Vec<Starter>>(include_str!(
        "../resources/expanded-strains.json"
    ))?);
    packs.push(Catalog {
        id: "cannabis".into(),
        name: "Cannabis".into(),
        description: "The existing cannabis strain collection and its recorded ancestry.".into(),
        entries: cannabis,
    });
    Ok(packs)
}
pub(crate) fn validate_selection(ids: &[String]) -> std::result::Result<(), &'static str> {
    if ids.len() > 6
        || ids.iter().any(|id| {
            !matches!(
                id.as_str(),
                "vegetables" | "herbs" | "flowers" | "fruit" | "houseplants" | "cannabis"
            )
        })
    {
        return Err("Choose from the six available starter collections");
    }
    Ok(())
}
pub async fn list() -> Result<Json<Vec<CatalogInfo>>> {
    Ok(Json(
        catalogs()?
            .into_iter()
            .map(|c| CatalogInfo {
                count: c.entries.len(),
                examples: c.entries.iter().take(3).map(|s| s.name.clone()).collect(),
                id: c.id,
                name: c.name,
                description: c.description,
            })
            .collect(),
    ))
}
async fn imported(tx: &mut Transaction, garden: &str) -> anyhow::Result<HashSet<String>> {
    let rows: Vec<db::Record> =
        db::query_as("SELECT catalog_id FROM garden_catalog_imports WHERE garden_id=?")
            .bind(garden)
            .fetch_all(&mut *tx)
            .await?;
    let mut ids = rows
        .iter()
        .map(|r| r.get("catalog_id"))
        .collect::<anyhow::Result<HashSet<String>>>()?;
    // Covers original-garden imports performed after migration 9 on a fresh install.
    if db::query_scalar::<i64>(
        "SELECT COUNT(*) FROM strain_catalog_imports WHERE garden_id=? AND catalog_version>=2",
    )
    .bind(garden)
    .fetch_one(tx)
    .await?
        > 0
    {
        ids.insert("cannabis".into());
    }
    Ok(ids)
}
pub async fn imports(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
) -> Result<Json<Vec<String>>> {
    let mut tx = app.pool.begin().await?;
    let mut ids = imported(&mut tx, &garden)
        .await?
        .into_iter()
        .collect::<Vec<_>>();
    tx.commit().await?;
    ids.sort();
    Ok(Json(ids))
}
pub(crate) struct ImportPlan {
    packs: Vec<Catalog>,
    total: usize,
}
impl ImportPlan {
    pub(crate) fn fits(&self) -> bool {
        self.total <= 1000
    }
    pub(crate) async fn apply(
        self,
        tx: &mut Transaction,
        garden: &str,
    ) -> anyhow::Result<ImportResult> {
        anyhow::ensure!(self.fits(), "Garden collection limit exceeded");
        let mut result = ImportResult {
            added: 0,
            imported: Vec::new(),
        };
        for pack in self.packs {
            result.added += strains::insert_catalog(tx, garden, &pack.entries, false).await?;
            db::query("INSERT INTO garden_catalog_imports(garden_id,catalog_id,imported_at) VALUES(?,?,?)")
                .bind(garden).bind(&pack.id).bind(Utc::now().timestamp()).execute(&mut *tx).await?;
            result.imported.push(pack.id);
        }
        Ok(result)
    }
}
/// Caller has created the garden in this transaction, or holds its write lock.
pub(crate) async fn prepare(
    tx: &mut Transaction,
    garden: &str,
    ids: &[String],
) -> anyhow::Result<ImportPlan> {
    let done = imported(tx, garden).await?;
    let packs = catalogs()?
        .into_iter()
        .filter(|c| ids.contains(&c.id) && !done.contains(&c.id))
        .collect::<Vec<_>>();
    let rows: Vec<db::Record> = db::query_as("SELECT name_key FROM strains WHERE garden_id=?")
        .bind(garden)
        .fetch_all(&mut *tx)
        .await?;
    let mut names = rows
        .iter()
        .map(|r| r.get("name_key"))
        .collect::<anyhow::Result<HashSet<String>>>()?;
    for pack in &packs {
        for entry in &pack.entries {
            names.insert(strains::name_key(&entry.name));
        }
    }
    Ok(ImportPlan {
        packs,
        total: names.len(),
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportInput {
    pub catalogs: Vec<String>,
}
#[derive(Serialize)]
pub struct ImportResult {
    pub added: usize,
    pub imported: Vec<String>,
}
pub async fn import(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Json(input): Json<ImportInput>,
) -> Result<Json<ImportResult>> {
    validate_selection(&input.catalogs).map_err(ApiError::bad)?;
    let mut tx = app.pool.begin().await?;
    strains::lock_garden(&mut tx, &garden).await?;
    let plan = prepare(&mut tx, &garden, &input.catalogs).await?;
    if !plan.fits() {
        return Err(ApiError::bad("These collections would exceed the garden's 1,000-card limit. Choose fewer collections or remove unused cards first."));
    }
    let result = plan.apply(&mut tx, &garden).await?;
    tx.commit().await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_catalogs_have_unique_names_sources_and_valid_parent_graphs() {
        let packs = catalogs().unwrap();
        assert_eq!(packs.len(), 6);
        let mut all = HashSet::new();
        for pack in &packs {
            let names = pack
                .entries
                .iter()
                .map(|e| e.name.as_str())
                .collect::<HashSet<_>>();
            assert!(!names.is_empty());
            for entry in &pack.entries {
                assert!(
                    all.insert(strains::name_key(&entry.name)),
                    "Duplicate {}",
                    entry.name
                );
                assert!(entry.name.len() <= 120 && !entry.species.is_empty());
                assert!(
                    entry.source_url.starts_with("https://")
                        || (pack.id == "cannabis" && entry.source_url.is_empty())
                );
                assert!(entry.parents.len() <= 2);
                let mut stack = entry.parents.iter().map(String::as_str).collect::<Vec<_>>();
                let mut seen = HashSet::new();
                while let Some(parent) = stack.pop() {
                    assert_ne!(parent, entry.name, "Catalog ancestry cycle");
                    assert!(names.contains(parent), "Missing parent {parent}");
                    if seen.insert(parent) {
                        stack.extend(
                            pack.entries
                                .iter()
                                .find(|e| e.name == parent)
                                .unwrap()
                                .parents
                                .iter()
                                .map(String::as_str),
                        );
                    }
                }
            }
        }
        assert_eq!(all.len(), 220);
    }
}
