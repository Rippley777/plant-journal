use crate::models::*;
use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Sqlite, SqlitePool, Transaction,
};
use std::{path::Path, time::Duration};
use uuid::Uuid;

pub fn id() -> String {
    Uuid::new_v4().to_string()
}
pub async fn open(path: &Path) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}
pub async fn settings(pool: &SqlitePool) -> anyhow::Result<Settings> {
    Ok(
        sqlx::query_as("SELECT timezone, photo_enabled, photo_time FROM settings WHERE id=1")
            .fetch_one(pool)
            .await?,
    )
}
pub async fn event(
    tx: &mut Transaction<'_, Sqlite>,
    kind: &str,
    title: &str,
    time: i64,
    entity: Option<&str>,
    detail: &str,
    plants: &[String],
) -> anyhow::Result<String> {
    let event_id = id();
    sqlx::query(
        "INSERT INTO events(id,kind,title,occurred_at,entity_id,detail) VALUES(?,?,?,?,?,?)",
    )
    .bind(&event_id)
    .bind(kind)
    .bind(title)
    .bind(time)
    .bind(entity)
    .bind(detail)
    .execute(&mut **tx)
    .await?;
    for plant in plants {
        sqlx::query("INSERT OR IGNORE INTO event_plants(event_id,plant_id) VALUES(?,?)")
            .bind(&event_id)
            .bind(plant)
            .execute(&mut **tx)
            .await?;
    }
    Ok(event_id)
}
pub async fn log_event(
    pool: &SqlitePool,
    kind: &str,
    title: &str,
    time: i64,
    entity: Option<&str>,
    detail: &str,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    event(&mut tx, kind, title, time, entity, detail, &[]).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn health(
    pool: &SqlitePool,
    component: &str,
    now: i64,
    error: Option<&str>,
) -> anyhow::Result<()> {
    let previous: Option<String> =
        sqlx::query_scalar("SELECT last_error FROM health WHERE component=?")
            .bind(component)
            .fetch_optional(pool)
            .await?
            .flatten();
    sqlx::query("INSERT INTO health(component,last_success,last_error,checked_at) VALUES(?,?,?,?) ON CONFLICT(component) DO UPDATE SET last_success=COALESCE(excluded.last_success,health.last_success),last_error=excluded.last_error,checked_at=excluded.checked_at")
        .bind(component).bind(if error.is_none() { Some(now) } else { None }).bind(error).bind(now).execute(pool).await?;
    if let Some(error) = error {
        if previous.as_deref() != Some(error) {
            log_event(
                pool,
                "failure",
                &format!("{component} unavailable"),
                now,
                None,
                error,
            )
            .await?;
        }
    } else if previous.is_some() {
        log_event(
            pool,
            "system",
            &format!("{component} recovered"),
            now,
            None,
            "Connection restored",
        )
        .await?;
    }
    Ok(())
}
pub fn month_bounds(month: &str, timezone: Tz) -> anyhow::Result<(i64, i64)> {
    let start = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")?;
    let next = if start.month() == 12 {
        NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1)
    }
    .ok_or_else(|| anyhow::anyhow!("Invalid month"))?;
    let to_timestamp = |date: NaiveDate| -> anyhow::Result<i64> {
        // A handful of timezones skip midnight. Use the first valid minute of that date.
        for minute in 0..1440 {
            if let Some(dt) = timezone
                .from_local_datetime(&date.and_hms_opt(minute / 60, minute % 60, 0).unwrap())
                .earliest()
            {
                return Ok(dt.timestamp());
            }
        }
        anyhow::bail!("Calendar date does not exist in this timezone")
    };
    Ok((to_timestamp(start)?, to_timestamp(next)?))
}
pub async fn calendar(
    pool: &SqlitePool,
    month: &str,
    plant: Option<&str>,
    kind: Option<&str>,
) -> anyhow::Result<Vec<Event>> {
    let setting = settings(pool).await?;
    let tz: Tz = setting.timezone.parse()?;
    let (start, end) = month_bounds(month, tz)?;
    let mut events: Vec<Event> = sqlx::query_as("SELECT e.* FROM events e WHERE occurred_at>=? AND occurred_at<? AND (? IS NULL OR kind=?) AND (? IS NULL OR EXISTS(SELECT 1 FROM event_plants p WHERE p.event_id=e.id AND p.plant_id=?)) ORDER BY occurred_at")
        .bind(start).bind(end).bind(kind).bind(kind).bind(plant).bind(plant).fetch_all(pool).await?;
    for e in &mut events {
        e.plant_ids = sqlx::query_scalar("SELECT plant_id FROM event_plants WHERE event_id=?")
            .bind(&e.id)
            .fetch_all(pool)
            .await?;
    }
    if plant.is_none() && (kind.is_none() || kind == Some("environment")) {
        let readings: Vec<Reading> = sqlx::query_as("SELECT recorded_at,temperature_c,humidity_percent FROM readings WHERE recorded_at>=? AND recorded_at<? ORDER BY recorded_at").bind(start).bind(end).fetch_all(pool).await?;
        let mut days: std::collections::BTreeMap<String, (i64, f64, f64, usize)> =
            std::collections::BTreeMap::new();
        for r in readings {
            if let Some(dt) = Utc.timestamp_opt(r.recorded_at, 0).single() {
                let day = dt.with_timezone(&tz).format("%Y-%m-%d").to_string();
                let entry = days.entry(day).or_insert((r.recorded_at, 0., 0., 0));
                entry.1 += r.temperature_c;
                entry.2 += r.humidity_percent;
                entry.3 += 1;
            }
        }
        for (day, (time, temp, humidity, count)) in days {
            events.push(Event {
                id: format!("environment-{day}"),
                kind: "environment".into(),
                title: format!(
                    "Daily average · {:.1}°C · {:.0}% RH",
                    temp / count as f64,
                    humidity / count as f64
                ),
                occurred_at: time,
                entity_id: None,
                detail: format!("{count} readings. Shared grow-space conditions."),
                plant_ids: vec![],
            });
        }
        events.sort_by_key(|e| e.occurred_at);
    }
    Ok(events)
}
