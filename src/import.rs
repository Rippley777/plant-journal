//! Explicit, one-time SQLite -> Azure SQL copy. The source is opened read-only.
use crate::database::{self as db, Database, Record};
use anyhow::Context;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{collections::BTreeMap, path::Path};

const TABLES: &[(&str, &str)] = &[
    ("strains", "id,garden_id,name,name_key,species,breeder,notes,status,lineage_note,source_url,created_at"),
    ("garden_catalog_imports", "garden_id,catalog_id,imported_at"),
    ("strain_catalog_imports", "garden_id,imported_at,catalog_version"),
    ("cross_plans", "id,garden_id,name,name_key,species,breeder,notes,parent_one_id,parent_two_id,converted_strain_id,converted_at,created_at,updated_at"),
    (
        "seeds",
        "id,name,variety,quantity,unit,supplier,purchase_year,storage_location,notes,created_at,strain_id,breeder,acquired_on,packet_code",
    ),
    ("germination_attempts", "id,garden_id,seed_id,started_on,seeds_sown,seeds_germinated,notes,created_at"),
    ("plants", "id,name,species,notes,archived,created_at,strain_id,seed_id,germination_id"),
    ("entries", "id,kind,body,occurred_at,created_at"),
    ("photos", "id,filename,captured_at,source"),
    ("readings", "recorded_at,temperature_c,humidity_percent"),
    (
        "devices",
        "id,name,role,adapter,address,channel,commanded_on,reported_on,checked_at,last_error",
    ),
    ("schedules", "device_id,enabled,start_time,end_time"),
    ("overrides", "device_id,on_state,expires_at"),
    ("events", "id,kind,title,occurred_at,entity_id,detail"),
    ("capture_runs", "local_date,status,attempted_at"),
    ("health", "component,last_success,last_error,checked_at"),
    ("entry_plants", "entry_id,plant_id"),
    ("photo_plants", "photo_id,plant_id"),
    ("photo_seeds", "photo_id,seed_id"),
    ("event_plants", "event_id,plant_id"),
    ("settings", "id,timezone,photo_enabled,photo_time"),
];

pub async fn sqlite_to_database(
    source: &Path,
    destination: &Database,
) -> anyhow::Result<BTreeMap<String, u64>> {
    let source = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(source).read_only(true))
        .await
        .context("Could not open the source SQLite database read-only")?;
    let mut snapshot = source.begin().await?;
    let mut tx = destination.begin().await?;
    if matches!(destination, Database::Azure(_)) {
        db::query("DECLARE @result int; EXEC @result=sys.sp_getapplock @Resource=N'plant-journal-import',@LockMode='Exclusive',@LockOwner='Transaction',@LockTimeout=15000; IF @result<0 THROW 50002,'Could not acquire import lock',1;").execute(&mut tx).await?;
    }
    // The legacy importer never flattens multiple users/gardens into one owner.
    let has_gardens: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='gardens'",
    )
    .fetch_one(&mut *snapshot)
    .await?;
    if has_gardens > 0 {
        let gardens: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM gardens")
            .fetch_one(&mut *snapshot)
            .await?;
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&mut *snapshot)
            .await?;
        anyhow::ensure!(gardens == 1 && users == 1, "Multi-user imports require a full backup/restore; the legacy importer will not merge accounts");
    }
    let gardens: i64 = db::query_scalar("SELECT COUNT(*) FROM gardens")
        .fetch_one(&mut tx)
        .await?;
    let users: i64 = db::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&mut tx)
        .await?;
    anyhow::ensure!(
        gardens == 1 && users == 1,
        "Destination has additional accounts or gardens; import refused"
    );
    // Do not merge two journals or overwrite existing data. The initial settings row is the sole exception.
    for (table, _) in TABLES.iter().filter(|(table, _)| *table != "settings") {
        let count: i64 = db::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&mut tx)
            .await?;
        anyhow::ensure!(
            count == 0,
            "Destination table {table} is not empty; import refused"
        );
    }
    let mut counts = BTreeMap::new();
    for (table, columns) in TABLES {
        // Older read-only source journals predate seed inventory or seed photos.
        if matches!(
            *table,
            "seeds"
                | "photo_seeds"
                | "strains"
                | "strain_catalog_imports"
                | "garden_catalog_imports"
                | "cross_plans"
                | "germination_attempts"
        ) {
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?",
            )
            .bind(*table)
            .fetch_one(&mut *snapshot)
            .await?;
            if exists == 0 {
                counts.insert(table.to_string(), 0);
                continue;
            }
        }
        let mut selected_columns = columns.to_string();
        if matches!(*table, "plants" | "seeds") {
            let defaults = if *table == "plants" {
                vec![
                    ("strain_id", "NULL"),
                    ("seed_id", "NULL"),
                    ("germination_id", "NULL"),
                ]
            } else {
                vec![
                    ("strain_id", "NULL"),
                    ("breeder", "''"),
                    ("acquired_on", "NULL"),
                    ("packet_code", "''"),
                ]
            };
            for (column, default) in defaults {
                let present: i64 = sqlx::query_scalar(&format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name=?"
                ))
                .bind(column)
                .fetch_one(&mut *snapshot)
                .await?;
                if present == 0 {
                    selected_columns = selected_columns
                        .split(',')
                        .map(|c| {
                            if c == column {
                                format!("{default} AS {column}")
                            } else {
                                c.to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                }
            }
        }

        if *table == "strain_catalog_imports" {
            let has_version: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pragma_table_info('strain_catalog_imports') WHERE name='catalog_version'",
            )
            .fetch_one(&mut *snapshot)
            .await?;
            if has_version == 0 {
                selected_columns =
                    selected_columns.replace("catalog_version", "1 AS catalog_version");
            }
        }
        let source_sql = if *table == "settings" && has_gardens > 0 {
            "SELECT 1 AS id,timezone,photo_enabled,photo_time FROM garden_settings".to_string()
        } else {
            format!("SELECT {selected_columns} FROM {table}")
        };
        let raw = sqlx::query(&source_sql).fetch_all(&mut *snapshot).await?;
        let records: Vec<Record> = raw
            .into_iter()
            .map(db::sqlite_row)
            .collect::<anyhow::Result<_>>()?;
        if *table == "settings" {
            anyhow::ensure!(
                records.len() == 1,
                "Source must contain exactly one settings row"
            );
            db::query("DELETE FROM settings").execute(&mut tx).await?;
        }
        let placeholders = vec!["?"; columns.split(',').count()].join(",");
        let sql = format!("INSERT INTO {table}({columns}) VALUES({placeholders})");
        let count = records.len() as u64;
        for record in records {
            let mut query = db::query(&sql);
            for value in record.values {
                query = query.bind(value);
            }
            query.execute(&mut tx).await.with_context(|| {
                format!("Could not import {table}; destination transaction will be rolled back")
            })?;
        }
        counts.insert(table.to_string(), count);
    }
    // Restore edges after all nodes exist; catalog order is not a topological order.
    if counts.get("strains").copied().unwrap_or(0) > 0 {
        let rows = sqlx::query("SELECT id,parent_one_id,parent_two_id FROM strains")
            .fetch_all(&mut *snapshot)
            .await?;
        for row in rows {
            let row = db::sqlite_row(row)?;
            db::query("UPDATE strains SET parent_one_id=?,parent_two_id=? WHERE id=?")
                .bind(row.get::<Option<String>>("parent_one_id")?)
                .bind(row.get::<Option<String>>("parent_two_id")?)
                .bind(row.get::<String>("id")?)
                .execute(&mut tx)
                .await?;
        }
    }
    // A migrated installation must not immediately operate physical equipment.
    db::query("UPDATE schedules SET enabled=0")
        .execute(&mut tx)
        .await?;
    db::query("DELETE FROM overrides").execute(&mut tx).await?;
    db::query("UPDATE settings SET photo_enabled=0")
        .execute(&mut tx)
        .await?;
    db::query(
        "UPDATE devices SET commanded_on=NULL,reported_on=NULL,checked_at=NULL,last_error=NULL",
    )
    .execute(&mut tx)
    .await?;
    db::query("UPDATE garden_settings SET timezone=(SELECT timezone FROM settings WHERE id=1),photo_time=(SELECT photo_time FROM settings WHERE id=1),photo_enabled=0 WHERE garden_id=?").bind(crate::auth::LEGACY_GARDEN).execute(&mut tx).await?;
    tx.commit().await.context("Import commit could not be confirmed. Inspect destination row counts before attempting another import; do not clear the source.")?;
    snapshot.commit().await?;
    source.close().await;
    Ok(counts)
}
