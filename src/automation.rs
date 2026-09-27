use crate::database as db;
use crate::{adapters::validate_reading, models::*, store, App};
use anyhow::Context;
use chrono::{DateTime, NaiveTime, Timelike, Utc};
use chrono_tz::Tz;
use std::{sync::Arc, time::Duration};

pub fn parse_time(value: &str) -> anyhow::Result<NaiveTime> {
    anyhow::ensure!(value.len() == 5, "Use HH:MM for times");
    Ok(NaiveTime::parse_from_str(value, "%H:%M")?)
}
pub fn desired_state(
    schedule: &Schedule,
    manual: Option<&Override>,
    now: DateTime<Utc>,
    tz: Tz,
) -> Option<bool> {
    if let Some(manual) = manual.filter(|m| m.expires_at > now.timestamp()) {
        return Some(manual.on_state);
    }
    if !schedule.enabled {
        return None;
    }
    let start = parse_time(&schedule.start_time).ok()?;
    let end = parse_time(&schedule.end_time).ok()?;
    let local = now.with_timezone(&tz).time();
    Some(match start.cmp(&end) {
        std::cmp::Ordering::Less => local >= start && local < end,
        std::cmp::Ordering::Greater => local >= start || local < end,
        std::cmp::Ordering::Equal => false,
    })
}
pub fn reading_stale(reading: Option<&Reading>, now: i64) -> bool {
    reading.is_none_or(|r| now - r.recorded_at >= 300 || r.recorded_at > now + 60)
}
pub fn due_photo(settings: &Settings, now: DateTime<Utc>) -> anyhow::Result<Option<String>> {
    if !settings.photo_enabled {
        return Ok(None);
    }
    let tz: Tz = settings.timezone.parse()?;
    let local = now.with_timezone(&tz);
    let scheduled = parse_time(&settings.photo_time)?;
    // Only the scheduled minute qualifies. No catch-up after downtime or a DST gap.
    Ok(
        (local.hour() == scheduled.hour() && local.minute() == scheduled.minute())
            .then(|| local.format("%Y-%m-%d").to_string()),
    )
}
pub async fn sample(app: &App, now: i64) -> anyhow::Result<()> {
    let Some(sensor) = &app.sensor else {
        return Ok(());
    };
    let result = tokio::time::timeout(Duration::from_secs(10), sensor.read(now))
        .await
        .context("Sensor timed out")
        .and_then(|r| r)
        .and_then(|r| {
            validate_reading(&r)?;
            Ok(r)
        });
    match result {
        Ok(r) => {
            db::query(
                "INSERT INTO readings(recorded_at,temperature_c,humidity_percent) VALUES(?,?,?)",
            )
            .bind(now)
            .bind(r.temperature_c)
            .bind(r.humidity_percent)
            .execute(&app.pool)
            .await?;
            store::health(&app.pool, "sensor", now, None).await?;
        }
        Err(error) => {
            store::health(&app.pool, "sensor", now, Some(&error.to_string())).await?;
        }
    }
    Ok(())
}
pub async fn capture(
    app: &App,
    now: i64,
    plants: Vec<String>,
    local_date: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let _lock = app.capture_lock.lock().await;
    let camera = app
        .camera
        .as_ref()
        .context("Camera is disabled in configuration")?;
    if let Some(date) = local_date {
        let inserted = db::query("INSERT OR IGNORE INTO capture_runs(local_date,status,attempted_at) VALUES(?,'attempted',?)").sql_server("INSERT INTO capture_runs(local_date,status,attempted_at) SELECT @P1,'attempted',@P2 WHERE NOT EXISTS(SELECT 1 FROM capture_runs WITH (UPDLOCK,HOLDLOCK) WHERE local_date=@P1)").bind(date).bind(now).execute(&app.pool).await?.rows_affected();
        if inserted == 0 {
            return Ok(None);
        }
    }
    let id = store::id();
    let filename = format!("{id}.{}", camera.extension());
    let folder = app.config.data_dir.join("photos");
    let temp = folder.join(format!(".{id}.part"));
    let destination = folder.join(&filename);
    let result: anyhow::Result<()> = async {
        tokio::time::timeout(Duration::from_secs(30), camera.capture(&temp))
            .await
            .context("Camera timed out")??;
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&temp)
            .await?;
        anyhow::ensure!(
            file.metadata().await?.len() > 0,
            "Camera produced an empty file"
        );
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&temp, &destination).await?;
        let mut tx = app.pool.begin().await?;
        db::query("INSERT INTO photos(id,filename,captured_at,source) VALUES(?,?,?,?)")
            .bind(&id)
            .bind(&filename)
            .bind(now)
            .bind(camera.source())
            .execute(&mut tx)
            .await?;
        for plant in &plants {
            db::query("INSERT OR IGNORE INTO photo_plants(photo_id,plant_id) VALUES(?,?)").sql_server("INSERT INTO photo_plants(photo_id,plant_id) SELECT @P1,@P2 WHERE NOT EXISTS(SELECT 1 FROM photo_plants WITH (UPDLOCK,HOLDLOCK) WHERE photo_id=@P1 AND plant_id=@P2)")
                .bind(&id)
                .bind(plant)
                .execute(&mut tx)
                .await?;
        }
        store::event(
            &mut tx,
            "photo",
            "Grow-space photo",
            now,
            Some(&id),
            camera.source(),
            &plants,
        )
        .await?;
        if let Some(date) = local_date {
            db::query("UPDATE capture_runs SET status='complete' WHERE local_date=?")
                .bind(date)
                .execute(&mut tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            store::health(&app.pool, "camera", now, None).await?;
            Ok(Some(id))
        }
        Err(error) => {
            let _ = tokio::fs::remove_file(&temp).await;
            // A remote COMMIT can succeed even if its acknowledgement is lost.
            // Retain finalized images on database failure to avoid broken committed photo records.
            if destination.exists() {
                tracing::warn!(photo_id=%id, "Retained finalized photo after an uncertain database write; reconcile against photo records before cleanup");
            }
            if let Some(date) = local_date {
                db::query("UPDATE capture_runs SET status='failed' WHERE local_date=?")
                    .bind(date)
                    .execute(&app.pool)
                    .await?;
            }
            store::health(&app.pool, "camera", now, Some(&error.to_string())).await?;
            Err(error)
        }
    }
}
pub async fn photo_tick(app: &App, now: DateTime<Utc>) -> anyhow::Result<()> {
    let settings = store::settings(&app.pool).await?;
    if let Some(date) = due_photo(&settings, now)? {
        let plants: Vec<String> = db::query_scalar("SELECT id FROM plants WHERE archived=0 AND garden_id='00000000-0000-0000-0000-000000000001'")
            .fetch_all(&app.pool)
            .await?;
        capture(app, now.timestamp(), plants, Some(&date)).await?;
    }
    Ok(())
}
pub async fn reconcile(app: &App, now: DateTime<Utc>) -> anyhow::Result<()> {
    let _lock = app.control_lock.lock().await;
    let tz: Tz = store::settings(&app.pool).await?.timezone.parse()?;
    let devices: Vec<Device> = db::query_as(
        "SELECT * FROM devices WHERE garden_id='00000000-0000-0000-0000-000000000001'",
    )
    .fetch_all(&app.pool)
    .await?;
    for d in devices {
        let schedule: Schedule = db::query_as("SELECT * FROM schedules WHERE device_id=?")
            .bind(&d.id)
            .fetch_one(&app.pool)
            .await?;
        let manual: Option<Override> = db::query_as("SELECT * FROM overrides WHERE device_id=?")
            .bind(&d.id)
            .fetch_optional(&app.pool)
            .await?;
        let expired = manual
            .as_ref()
            .is_some_and(|m| m.expires_at <= now.timestamp());
        // With no enabled schedule, an expired manual run explicitly returns the outlet to off.
        let desired = desired_state(&schedule, manual.as_ref(), now, tz).or(if expired {
            Some(false)
        } else {
            None
        });
        let driver = if d.adapter == "shelly" {
            &app.shelly_switch
        } else {
            &app.simulated_switch
        };
        let mut attempted = false;
        let result: anyhow::Result<bool> = async {
            let current = tokio::time::timeout(Duration::from_secs(5), driver.status(&d))
                .await
                .context("Outlet status timed out")??;
            if let Some(on) = desired {
                if current != on {
                    attempted = true;
                    // Persist intent before talking to the outlet, including unsuccessful commands.
                    db::query("UPDATE devices SET commanded_on=? WHERE id=?")
                        .bind(on)
                        .bind(&d.id)
                        .execute(&app.pool)
                        .await?;
                    store::log_event(
                        &app.pool,
                        "device",
                        &format!("{} · requested {}", d.name, if on { "on" } else { "off" }),
                        now.timestamp(),
                        Some(&d.id),
                        "Command issued; output confirmation follows",
                    )
                    .await?;
                    tokio::time::timeout(Duration::from_secs(5), driver.set(&d, on))
                        .await
                        .context("Outlet command timed out")??;
                    let reported = tokio::time::timeout(Duration::from_secs(5), driver.status(&d))
                        .await
                        .context("Outlet verification timed out")??;
                    anyhow::ensure!(reported == on, "Outlet did not confirm requested state");
                    return Ok(reported);
                }
            }
            Ok(current)
        }
        .await;
        match result {
            Ok(on) => {
                db::query(
                    "UPDATE devices SET reported_on=?,checked_at=?,last_error=NULL WHERE id=?",
                )
                .bind(on)
                .bind(now.timestamp())
                .bind(&d.id)
                .execute(&app.pool)
                .await?;
                if attempted || d.reported_on != Some(on) || d.last_error.is_some() {
                    store::log_event(&app.pool,"device",&format!("{} · output {}",d.name,if on {"on"} else {"off"}),now.timestamp(),Some(&d.id),"Device-reported outlet state; equipment operation is not independently measured").await?;
                }
                if expired {
                    db::query("DELETE FROM overrides WHERE device_id=? AND expires_at<=?")
                        .bind(&d.id)
                        .bind(now.timestamp())
                        .execute(&app.pool)
                        .await?;
                }
            }
            Err(error) => {
                let message = error.to_string();
                db::query(
                    "UPDATE devices SET reported_on=NULL,checked_at=?,last_error=? WHERE id=?",
                )
                .bind(now.timestamp())
                .bind(&message)
                .bind(&d.id)
                .execute(&app.pool)
                .await?;
                if d.last_error.as_deref() != Some(&message) {
                    store::log_event(
                        &app.pool,
                        "failure",
                        &format!("{} unavailable", d.name),
                        now.timestamp(),
                        Some(&d.id),
                        &message,
                    )
                    .await?;
                }
            }
        }
    }
    Ok(())
}
pub fn spawn(app: Arc<App>) -> Vec<tokio::task::JoinHandle<()>> {
    if !app.config.automation_enabled {
        tracing::info!("Hardware automation is disabled for this instance");
        return vec![];
    }
    let mut tasks = vec![];
    for worker in 0..3 {
        let app = app.clone();
        tasks.push(tokio::spawn(async move {
            let mut timer =
                tokio::time::interval(Duration::from_secs(if worker == 0 { 60 } else { 10 }));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                timer.tick().await;
                let now = Utc::now();
                let result = match worker {
                    0 => sample(&app, now.timestamp()).await,
                    1 => photo_tick(&app, now).await,
                    _ => reconcile(&app, now).await,
                };
                if let Err(error) = result {
                    tracing::error!(worker,%error,"Background task failed");
                }
            }
        }));
    }
    tasks
}
