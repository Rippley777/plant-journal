//! Explicit, one-time SQLite -> Azure SQL copy. The source is opened read-only.
use crate::database::{self as db, Database, Record};
use anyhow::Context;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::{collections::BTreeMap, path::Path};

const TABLES: &[(&str, &str)] = &[
    (
        "seeds",
        "id,name,variety,quantity,unit,supplier,purchase_year,storage_location,notes,created_at",
    ),
    ("plants", "id,name,species,notes,archived,created_at"),
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
        // Older read-only source journals predate seed inventory.
        if *table == "seeds" {
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='seeds'",
            )
            .fetch_one(&mut *snapshot)
            .await?;
            if exists == 0 {
                counts.insert(table.to_string(), 0);
                continue;
            }
        }
        let raw = sqlx::query(&format!("SELECT {columns} FROM {table}"))
            .fetch_all(&mut *snapshot)
            .await?;
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
    tx.commit().await.context("Import commit could not be confirmed. Inspect destination row counts before attempting another import; do not clear the source.")?;
    snapshot.commit().await?;
    source.close().await;
    Ok(counts)
}
