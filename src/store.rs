use crate::database::{self as db, Database, Transaction};
use crate::models::*;
use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

pub fn id() -> String {
    Uuid::new_v4().to_string()
}
pub async fn settings(pool: &Database) -> anyhow::Result<Settings> {
    db::query_as("SELECT timezone, photo_enabled, photo_time FROM settings WHERE id=1")
        .fetch_one(pool)
        .await
}
pub async fn event(
    tx: &mut Transaction,
    kind: &str,
    title: &str,
    time: i64,
    entity: Option<&str>,
    detail: &str,
    plants: &[String],
) -> anyhow::Result<String> {
    let event_id = id();
    db::query("INSERT INTO events(id,kind,title,occurred_at,entity_id,detail) VALUES(?,?,?,?,?,?)")
        .bind(&event_id)
        .bind(kind)
        .bind(title)
        .bind(time)
        .bind(entity)
        .bind(detail)
        .execute(&mut *tx)
        .await?;
    for plant in plants {
        db::query("INSERT OR IGNORE INTO event_plants(event_id,plant_id) VALUES(?,?)").sql_server("INSERT INTO event_plants(event_id,plant_id) SELECT @P1,@P2 WHERE NOT EXISTS(SELECT 1 FROM event_plants WITH (UPDLOCK,HOLDLOCK) WHERE event_id=@P1 AND plant_id=@P2)")
            .bind(&event_id)
            .bind(plant)
            .execute(&mut *tx)
            .await?;
    }
    Ok(event_id)
}
pub async fn log_event(
    pool: &Database,
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
    pool: &Database,
    component: &str,
    now: i64,
    error: Option<&str>,
) -> anyhow::Result<()> {
    let previous: Option<String> =
        db::query_scalar("SELECT last_error FROM health WHERE component=?")
            .bind(component)
            .fetch_optional(pool)
            .await?
            .flatten();
    db::query("INSERT INTO health(component,last_success,last_error,checked_at) VALUES(?,?,?,?) ON CONFLICT(component) DO UPDATE SET last_success=COALESCE(excluded.last_success,health.last_success),last_error=excluded.last_error,checked_at=excluded.checked_at").sql_server("MERGE health WITH (HOLDLOCK) AS target USING (SELECT @P1 component,@P2 last_success,@P3 last_error,@P4 checked_at) AS src ON target.component=src.component WHEN MATCHED THEN UPDATE SET last_success=COALESCE(src.last_success,target.last_success),last_error=src.last_error,checked_at=src.checked_at WHEN NOT MATCHED THEN INSERT(component,last_success,last_error,checked_at) VALUES(src.component,src.last_success,src.last_error,src.checked_at);")
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
    pool: &Database,
    month: &str,
    plant: Option<&str>,
    kind: Option<&str>,
) -> anyhow::Result<Vec<Event>> {
    let setting = settings(pool).await?;
    let tz: Tz = setting.timezone.parse()?;
    let (start, end) = month_bounds(month, tz)?;
    let mut events: Vec<Event> = db::query_as("SELECT e.* FROM events e WHERE occurred_at>=? AND occurred_at<? AND (? IS NULL OR kind=?) AND (? IS NULL OR EXISTS(SELECT 1 FROM event_plants p WHERE p.event_id=e.id AND p.plant_id=?)) ORDER BY occurred_at")
        .bind(start).bind(end).bind(kind).bind(kind).bind(plant).bind(plant).fetch_all(pool).await?;
    let mut links = plant_links(
        pool,
        LinkKind::Event,
        &events.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
    )
    .await?;
    for event in &mut events {
        event.plant_ids = links.remove(&event.id).unwrap_or_default();
    }
    if plant.is_none() && (kind.is_none() || kind == Some("environment")) {
        let readings: Vec<Reading> = db::query_as("SELECT recorded_at,temperature_c,humidity_percent FROM readings WHERE recorded_at>=? AND recorded_at<? ORDER BY recorded_at").bind(start).bind(end).fetch_all(pool).await?;
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

/// Fetch associations in bounded batches instead of one hosted-database round trip per item.
pub enum LinkKind {
    Entry,
    Photo,
    Event,
}
pub async fn plant_links(
    pool: &Database,
    kind: LinkKind,
    ids: &[String],
) -> anyhow::Result<std::collections::HashMap<String, Vec<String>>> {
    let (table, key) = match kind {
        LinkKind::Entry => ("entry_plants", "entry_id"),
        LinkKind::Photo => ("photo_plants", "photo_id"),
        LinkKind::Event => ("event_plants", "event_id"),
    };
    let mut links = std::collections::HashMap::<String, Vec<String>>::new();
    for chunk in ids.chunks(500) {
        let sql = format!(
            "SELECT {key} AS entity_id,plant_id FROM {table} WHERE {key} IN ({})",
            vec!["?"; chunk.len()].join(",")
        );
        let mut query = db::query_as::<db::Record>(&sql);
        for id in chunk {
            query = query.bind(id);
        }
        for row in query.fetch_all(pool).await? {
            links
                .entry(row.get("entity_id")?)
                .or_default()
                .push(row.get("plant_id")?);
        }
    }
    Ok(links)
}
