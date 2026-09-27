use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use chrono::{DateTime, Utc};
use chrono_tz::America::Chicago;
use http_body_util::BodyExt;
use plant_journal::database as db;
use plant_journal::{
    adapters::{Camera, IioSensor, Sensor, Switch},
    api, automation,
    config::Config,
    models::*,
    store, App,
};
use serde_json::{json, Value};
use std::{path::Path, sync::Arc};
use tempfile::TempDir;
use tower::ServiceExt;

async fn setup() -> (TempDir, Arc<App>, Router) {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        data_dir: dir.path().into(),
        ..Config::default()
    };
    let app = App::open(config).await.unwrap();
    let router = api::router(app.clone());
    (dir, app, router)
}
async fn request(
    router: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost");
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    let response = router
        .clone()
        .oneshot(
            req.body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes).to_string()));
    (status, value)
}
async fn add_plant(router: &Router, name: &str) -> String {
    let (status, value) =
        request(router, "POST", "/api/v1/plants", Some(json!({"name":name}))).await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    value["id"].as_str().unwrap().into()
}
async fn add_device(router: &Router) -> String {
    let (status, value) = request(
        router,
        "POST",
        "/api/v1/devices",
        Some(json!({"name":"Tent light","role":"light","adapter":"simulated"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    value["id"].as_str().unwrap().into()
}
fn utc(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}
fn schedule(start: &str, end: &str) -> Schedule {
    Schedule {
        device_id: "test".into(),
        enabled: true,
        start_time: start.into(),
        end_time: end.into(),
    }
}

#[tokio::test]
async fn pages_and_assets_are_served_without_external_dependencies() {
    let (_dir, _app, router) = setup().await;
    for path in [
        "/",
        "/plants",
        "/journal",
        "/calendar",
        "/photos",
        "/environment",
        "/equipment",
        "/settings",
        "/assets/app.css",
        "/assets/app.js",
    ] {
        let (status, body) = request(&router, "GET", path, None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert!(!body.as_str().unwrap().contains("https://fonts"));
    }
}
#[tokio::test]
async fn journal_links_edit_archive_and_restart_preserve_history() {
    let (dir, app, router) = setup().await;
    let a = add_plant(&router, "Monstera").await;
    let b = add_plant(&router, "Pothos").await;
    let time = utc("2026-09-12T18:00:00Z").timestamp();
    let (status,entry)=request(&router,"POST","/api/v1/entries",Some(json!({"kind":"watering","body":"Watered by hand","occurred_at":time,"plant_ids":[a,b]}))).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = entry["id"].as_str().unwrap();
    for plant in [&a, &b] {
        let (_, events) = request(
            &router,
            "GET",
            &format!("/api/v1/calendar?month=2026-09&kind=watering&plant={plant}"),
            None,
        )
        .await;
        assert_eq!(events.as_array().unwrap().len(), 1);
    }
    let (status,_)=request(&router,"PUT",&format!("/api/v1/entries/{id}"),Some(json!({"kind":"note","body":"Actually just checked the soil","occurred_at":time,"plant_ids":[a]}))).await;
    assert_eq!(status, StatusCode::OK);
    let (_, entries) = request(&router, "GET", &format!("/api/v1/entries?plant={b}"), None).await;
    assert_eq!(entries, json!([]));
    let (status, _) = request(
        &router,
        "PUT",
        &format!("/api/v1/plants/{a}"),
        Some(json!({"name":"Monstera","archived":true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    app.pool.close().await;
    let reopened = App::open(Config {
        data_dir: dir.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    let router = api::router(reopened);
    let (_, plant) = request(&router, "GET", &format!("/api/v1/plants/{a}"), None).await;
    assert_eq!(plant["archived"], true);
    let (_, entries) = request(&router, "GET", &format!("/api/v1/entries?plant={a}"), None).await;
    assert_eq!(entries[0]["body"], "Actually just checked the soil");
}
#[tokio::test]
async fn invalid_plant_associations_and_inputs_do_not_create_partial_records() {
    let (_dir, app, router) = setup().await;
    let a = add_plant(&router, "Fern").await;
    let (status,_)=request(&router,"POST","/api/v1/entries",Some(json!({"kind":"watering","body":"No phantom entries","occurred_at":1,"plant_ids":[a,"missing"]}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM entries")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let (status, _) = request(
        &router,
        "POST",
        "/api/v1/plants",
        Some(json!({"name":"   "})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = request(
        &router,
        "PUT",
        "/api/v1/settings",
        Some(json!({"timezone":"Mars/Olympus","photo_enabled":false,"photo_time":"12:00"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
#[tokio::test]
async fn calendar_respects_local_month_and_summarizes_local_days() {
    let (_dir, app, router) = setup().await;
    let a = add_plant(&router, "Fern").await;
    for (when, body) in [
        ("2026-03-01T05:59:00Z", "February locally"),
        ("2026-03-01T06:00:00Z", "March locally"),
    ] {
        let t = utc(when).timestamp();
        let (status, _) = request(
            &router,
            "POST",
            "/api/v1/entries",
            Some(json!({"kind":"note","body":body,"occurred_at":t,"plant_ids":[a]})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        db::query(
            "INSERT INTO readings(recorded_at,temperature_c,humidity_percent) VALUES(?,22,55)",
        )
        .bind(t)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    let events = store::calendar(&app.pool, "2026-03", None, Some("note"))
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].title, "March locally");
    let events = store::calendar(&app.pool, "2026-02", None, Some("environment"))
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, "environment-2026-02-28");
    assert!(
        store::calendar(&app.pool, "2026-02", Some(&a), Some("environment"))
            .await
            .unwrap()
            .is_empty()
    );
}
#[test]
fn schedules_handle_overnight_boundaries_and_disabled_state() {
    let s = schedule("20:00", "06:00");
    for (time, expected) in [
        ("2026-09-12T00:59:00Z", false),
        ("2026-09-12T01:00:00Z", true),
        ("2026-09-12T10:59:00Z", true),
        ("2026-09-12T11:00:00Z", false),
    ] {
        assert_eq!(
            automation::desired_state(&s, None, utc(time), Chicago),
            Some(expected)
        );
    }
    let mut s = s;
    s.enabled = false;
    assert_eq!(
        automation::desired_state(&s, None, Utc::now(), Chicago),
        None
    );
}
#[test]
fn schedules_follow_dst_wall_time_and_override_expiry_uses_utc() {
    let s = schedule("01:00", "02:00");
    for time in ["2026-11-01T06:30:00Z", "2026-11-01T07:30:00Z"] {
        assert_eq!(
            automation::desired_state(&s, None, utc(time), Chicago),
            Some(true)
        );
    }
    assert_eq!(
        automation::desired_state(&s, None, utc("2026-11-01T08:00:00Z"), Chicago),
        Some(false)
    );
    let s = schedule("02:30", "04:00");
    assert_eq!(
        automation::desired_state(&s, None, utc("2026-03-08T08:00:00Z"), Chicago),
        Some(true)
    );
    let expiry = utc("2026-11-01T07:00:00Z");
    let manual = Override {
        device_id: "test".into(),
        on_state: false,
        expires_at: expiry.timestamp(),
    };
    let s = schedule("01:00", "02:00");
    assert_eq!(
        automation::desired_state(
            &s,
            Some(&manual),
            expiry - chrono::Duration::seconds(1),
            Chicago
        ),
        Some(false)
    );
    assert_eq!(
        automation::desired_state(&s, Some(&manual), expiry, Chicago),
        Some(true)
    );
}
#[test]
fn photo_due_skips_missed_minutes_and_dst_gap() {
    let s = Settings {
        timezone: "America/Chicago".into(),
        photo_enabled: true,
        photo_time: "12:00".into(),
    };
    assert_eq!(
        automation::due_photo(&s, utc("2026-09-12T17:00:30Z")).unwrap(),
        Some("2026-09-12".into())
    );
    assert_eq!(
        automation::due_photo(&s, utc("2026-09-12T17:01:00Z")).unwrap(),
        None
    );
    let s = Settings {
        photo_time: "02:30".into(),
        ..s
    };
    assert_eq!(
        automation::due_photo(&s, utc("2026-03-08T08:30:00Z")).unwrap(),
        None
    );
}
#[tokio::test]
async fn photo_capture_is_atomic_shared_and_deduplicated_across_restart() {
    let (dir, app, router) = setup().await;
    let a = add_plant(&router, "A").await;
    let b = add_plant(&router, "B").await;
    let id = automation::capture(&app, 1000, vec![a.clone(), b.clone()], Some("2026-11-01"))
        .await
        .unwrap()
        .unwrap();
    assert!(automation::capture(&app, 500, vec![], Some("2026-11-01"))
        .await
        .unwrap()
        .is_none());
    let (_, photos) = request(&router, "GET", &format!("/api/v1/photos?plant={a}"), None).await;
    assert_eq!(photos.as_array().unwrap().len(), 1);
    assert_eq!(photos[0]["plant_ids"].as_array().unwrap().len(), 2);
    assert_eq!(
        std::fs::read_dir(dir.path().join("photos"))
            .unwrap()
            .count(),
        1
    );
    let (status, _) = request(&router, "GET", &format!("/api/v1/photos/{id}/image"), None).await;
    assert_eq!(status, StatusCode::OK);
    app.pool.close().await;
    let reopened = App::open(Config {
        data_dir: dir.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    assert!(
        automation::capture(&reopened, 1500, vec![], Some("2026-11-01"))
            .await
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn photo_links_and_explicit_deletion_update_calendar_and_files() {
    let (dir, app, router) = setup().await;
    let a = add_plant(&router, "A").await;
    let b = add_plant(&router, "B").await;
    let id = automation::capture(
        &app,
        utc("2026-09-12T18:00:00Z").timestamp(),
        vec![a.clone()],
        None,
    )
    .await
    .unwrap()
    .unwrap();
    let (status, _) = request(
        &router,
        "PUT",
        &format!("/api/v1/photos/{id}"),
        Some(json!({"plant_ids":[b]})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(
        store::calendar(&app.pool, "2026-09", Some(&a), Some("photo"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store::calendar(&app.pool, "2026-09", Some(&b), Some("photo"))
            .await
            .unwrap()
            .len(),
        1
    );
    let (status, _) = request(&router, "DELETE", &format!("/api/v1/photos/{id}"), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        std::fs::read_dir(dir.path().join("photos"))
            .unwrap()
            .count(),
        0
    );
}
struct BrokenCamera;
#[async_trait]
impl Camera for BrokenCamera {
    fn extension(&self) -> &'static str {
        "jpg"
    }
    fn source(&self) -> &'static str {
        "test"
    }
    async fn capture(&self, path: &Path) -> anyhow::Result<()> {
        tokio::fs::write(path, "partial").await?;
        anyhow::bail!("Camera disconnected")
    }
}
#[tokio::test]
async fn camera_failure_cleans_partial_file_records_failure_and_does_not_retry_daily_run() {
    let (dir, mut app, router) = setup().await;
    drop(router);
    Arc::get_mut(&mut app).unwrap().camera = Some(Arc::new(BrokenCamera));
    assert!(automation::capture(&app, 1000, vec![], Some("2026-09-12"))
        .await
        .is_err());
    assert_eq!(
        std::fs::read_dir(dir.path().join("photos"))
            .unwrap()
            .count(),
        0
    );
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM photos")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let error: String = db::query_scalar("SELECT last_error FROM health WHERE component='camera'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(error, "Camera disconnected");
    assert!(automation::capture(&app, 1100, vec![], Some("2026-09-12"))
        .await
        .unwrap()
        .is_none());
}
#[tokio::test]
async fn failed_image_write_never_creates_successful_photo() {
    let (dir, app, _router) = setup().await;
    std::fs::remove_dir(dir.path().join("photos")).unwrap();
    std::fs::write(dir.path().join("photos"), "not a directory").unwrap();
    assert!(automation::capture(&app, 1000, vec![], None).await.is_err());
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM events WHERE kind='photo'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
#[tokio::test]
async fn iio_conversion_and_malformed_readings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("in_temp_input"), "23500\n").unwrap();
    std::fs::write(dir.path().join("in_humidityrelative_input"), "56700\n").unwrap();
    let sensor = IioSensor {
        path: dir.path().into(),
    };
    let reading = sensor.read(1000).await.unwrap();
    assert_eq!(reading.temperature_c, 23.5);
    assert_eq!(reading.humidity_percent, 56.7);
    std::fs::write(dir.path().join("in_temp_input"), "NaN").unwrap();
    assert!(sensor.read(1000).await.is_err());
    std::fs::write(dir.path().join("in_temp_input"), "bad").unwrap();
    assert!(sensor.read(1000).await.is_err());
    assert!(!automation::reading_stale(Some(&reading), 1299));
    assert!(automation::reading_stale(Some(&reading), 1300));
    assert!(automation::reading_stale(None, 0));
    assert!(automation::reading_stale(Some(&reading), 500));
}
struct BrokenSensor;
#[async_trait]
impl Sensor for BrokenSensor {
    async fn read(&self, _: i64) -> anyhow::Result<Reading> {
        anyhow::bail!("Sensor unplugged")
    }
}
#[tokio::test]
async fn sensor_failure_preserves_last_good_reading_and_deduplicates_failure_events() {
    let (_dir, mut app, router) = setup().await;
    automation::sample(&app, 1000).await.unwrap();
    drop(router);
    Arc::get_mut(&mut app).unwrap().sensor = Some(Arc::new(BrokenSensor));
    automation::sample(&app, 1100).await.unwrap();
    automation::sample(&app, 1200).await.unwrap();
    let readings: Vec<Reading> =
        db::query_as("SELECT recorded_at,temperature_c,humidity_percent FROM readings")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].recorded_at, 1000);
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM events WHERE kind='failure'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
#[tokio::test]
async fn restart_reconciles_current_schedule_without_replaying_transitions() {
    let (dir, app, router) = setup().await;
    let id = add_device(&router).await;
    db::query(
        "UPDATE schedules SET enabled=1,start_time='08:00',end_time='20:00' WHERE device_id=?",
    )
    .bind(&id)
    .execute(&app.pool)
    .await
    .unwrap();
    automation::reconcile(&app, utc("2026-09-12T14:00:00Z"))
        .await
        .unwrap();
    let state: bool = db::query_scalar("SELECT reported_on FROM devices WHERE id=?")
        .bind(&id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(state);
    let driver = app.simulated_switch.clone();
    app.pool.close().await;
    let mut reopened = App::open(Config {
        data_dir: dir.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    Arc::get_mut(&mut reopened).unwrap().simulated_switch = driver;
    automation::reconcile(&reopened, utc("2026-09-15T03:00:00Z"))
        .await
        .unwrap();
    let state: bool = db::query_scalar("SELECT reported_on FROM devices WHERE id=?")
        .bind(&id)
        .fetch_one(&reopened.pool)
        .await
        .unwrap();
    assert!(!state);
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM events WHERE title LIKE '%requested%'")
        .fetch_one(&reopened.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    automation::reconcile(&reopened, utc("2026-09-15T03:01:00Z"))
        .await
        .unwrap();
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM events WHERE title LIKE '%requested%'")
        .fetch_one(&reopened.pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
}
#[tokio::test]
async fn disabled_schedule_does_not_switch_and_expired_override_returns_off() {
    let (_dir, app, router) = setup().await;
    let id = add_device(&router).await;
    let now = utc("2026-09-12T18:00:00Z");
    automation::reconcile(&app, now).await.unwrap();
    let command: Option<bool> = db::query_scalar("SELECT commanded_on FROM devices WHERE id=?")
        .bind(&id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(command, None);
    db::query("INSERT INTO overrides(device_id,on_state,expires_at) VALUES(?,1,?)")
        .bind(&id)
        .bind(now.timestamp() + 60)
        .execute(&app.pool)
        .await
        .unwrap();
    automation::reconcile(&app, now).await.unwrap();
    let state: bool = db::query_scalar("SELECT reported_on FROM devices WHERE id=?")
        .bind(&id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(state);
    automation::reconcile(&app, now + chrono::Duration::seconds(60))
        .await
        .unwrap();
    let state: bool = db::query_scalar("SELECT reported_on FROM devices WHERE id=?")
        .bind(&id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(!state);
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM overrides")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
struct HangingSwitch;
#[async_trait]
impl Switch for HangingSwitch {
    async fn status(&self, _: &Device) -> anyhow::Result<bool> {
        std::future::pending().await
    }
    async fn set(&self, _: &Device, _: bool) -> anyhow::Result<()> {
        Ok(())
    }
}
#[tokio::test]
async fn outlet_timeout_marks_state_unknown_and_preserves_pending_override() {
    let (_dir, mut app, router) = setup().await;
    let id = add_device(&router).await;
    drop(router);
    Arc::get_mut(&mut app).unwrap().simulated_switch = Arc::new(HangingSwitch);
    db::query("INSERT INTO overrides(device_id,on_state,expires_at) VALUES(?,1,0)")
        .bind(&id)
        .execute(&app.pool)
        .await
        .unwrap();
    automation::reconcile(&app, Utc::now()).await.unwrap();
    let d: Device = db::query_as("SELECT * FROM devices WHERE id=?")
        .bind(&id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(d.reported_on, None);
    assert!(d.last_error.unwrap().contains("timed out"));
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM overrides")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
#[tokio::test]
async fn backup_restore_retains_database_and_photo_associations() {
    let (dir, app, router) = setup().await;
    let plant = add_plant(&router, "Backed-up fern").await;
    let id = automation::capture(&app, 1000, vec![plant.clone()], None)
        .await
        .unwrap()
        .unwrap();
    app.pool.close().await;
    let restore = tempfile::tempdir().unwrap();
    std::fs::copy(
        dir.path().join("journal.sqlite3"),
        restore.path().join("journal.sqlite3"),
    )
    .unwrap();
    std::fs::create_dir(restore.path().join("photos")).unwrap();
    for file in std::fs::read_dir(dir.path().join("photos")).unwrap() {
        let file = file.unwrap();
        std::fs::copy(
            file.path(),
            restore.path().join("photos").join(file.file_name()),
        )
        .unwrap();
    }
    let app = App::open(Config {
        data_dir: restore.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    let router = api::router(app);
    let (status, _) = request(&router, "GET", &format!("/api/v1/photos/{id}/image"), None).await;
    assert_eq!(status, StatusCode::OK);
    let (_, photos) = request(
        &router,
        "GET",
        &format!("/api/v1/photos?plant={plant}"),
        None,
    )
    .await;
    assert_eq!(photos.as_array().unwrap().len(), 1);
}
#[tokio::test]
async fn rejects_cross_origin_mutation_and_form_posts() {
    let (_dir, _app, router) = setup().await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/plants")
        .header("host", "localhost")
        .header("origin", "http://evil.example")
        .header("content-type", "application/json")
        .body(Body::from("{\"name\":\"intruder\"}"))
        .unwrap();
    assert_eq!(
        router.clone().oneshot(req).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/plants")
        .header("host", "localhost")
        .body(Body::from("name=intruder"))
        .unwrap();
    assert_eq!(
        router.clone().oneshot(req).await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
}
#[tokio::test]
async fn shelly_rpc_commands_and_verification_use_local_http() {
    use axum::{extract::State, routing::post, Json};
    use std::sync::atomic::{AtomicBool, Ordering};
    async fn rpc(State(state): State<Arc<AtomicBool>>, Json(body): Json<Value>) -> Json<Value> {
        assert_eq!(body["params"]["id"], 0);
        match body["method"].as_str().unwrap() {
            "Switch.Set" => {
                state.store(body["params"]["on"].as_bool().unwrap(), Ordering::SeqCst);
                Json(json!({"id":1,"result":{"was_on":false}}))
            }
            "Switch.GetStatus" => {
                Json(json!({"id":1,"result":{"output":state.load(Ordering::SeqCst)}}))
            }
            _ => panic!("Unexpected RPC"),
        }
    }
    let state = Arc::new(AtomicBool::new(false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/rpc", post(rpc)).with_state(state),
        )
        .await
        .unwrap()
    });
    let (_dir, app, router) = setup().await;
    let(status,value)=request(&router,"POST","/api/v1/devices",Some(json!({"name":"Local fan","role":"fan","adapter":"shelly","address":format!("http://{address}")}))).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = value["id"].as_str().unwrap();
    db::query("INSERT INTO overrides(device_id,on_state,expires_at) VALUES(?,1,?)")
        .bind(id)
        .bind(Utc::now().timestamp() + 3600)
        .execute(&app.pool)
        .await
        .unwrap();
    automation::reconcile(&app, Utc::now()).await.unwrap();
    let d: Device = db::query_as("SELECT * FROM devices WHERE id=?")
        .bind(id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(d.commanded_on, Some(true));
    assert_eq!(d.reported_on, Some(true));
    server.abort();
}

#[tokio::test]
async fn editing_outlet_connection_disables_automation_and_clears_old_state() {
    let (_dir, app, router) = setup().await;
    let id = add_device(&router).await;
    db::query("UPDATE schedules SET enabled=1 WHERE device_id=?")
        .bind(&id)
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("INSERT INTO overrides(device_id,on_state,expires_at) VALUES(?,1,9999999999)")
        .bind(&id)
        .execute(&app.pool)
        .await
        .unwrap();
    let (status,_)=request(&router,"PUT",&format!("/api/v1/devices/{id}"),Some(json!({"name":"Actual light","role":"light","adapter":"shelly","address":"http://192.168.1.50"}))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let enabled: bool = db::query_scalar("SELECT enabled FROM schedules WHERE device_id=?")
        .bind(&id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(!enabled);
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM overrides")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let d: Device = db::query_as("SELECT * FROM devices WHERE id=?")
        .bind(&id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(d.reported_on, None);
    assert_eq!(d.commanded_on, None);
}

struct WaitingCamera {
    entered: tokio::sync::Notify,
}
#[async_trait]
impl Camera for WaitingCamera {
    fn extension(&self) -> &'static str {
        "jpg"
    }
    fn source(&self) -> &'static str {
        "test"
    }
    async fn capture(&self, _: &Path) -> anyhow::Result<()> {
        self.entered.notify_one();
        std::future::pending().await
    }
}
#[tokio::test]
async fn a_stuck_camera_does_not_block_equipment_control() {
    let (_dir, mut app, router) = setup().await;
    let id = add_device(&router).await;
    drop(router);
    let camera = Arc::new(WaitingCamera {
        entered: tokio::sync::Notify::new(),
    });
    Arc::get_mut(&mut app).unwrap().camera = Some(camera.clone());
    db::query("INSERT INTO overrides(device_id,on_state,expires_at) VALUES(?,1,?)")
        .bind(&id)
        .bind(Utc::now().timestamp() + 3600)
        .execute(&app.pool)
        .await
        .unwrap();
    let capture_app = app.clone();
    let task = tokio::spawn(async move {
        automation::capture(&capture_app, Utc::now().timestamp(), vec![], None).await
    });
    camera.entered.notified().await;
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        automation::reconcile(&app, Utc::now()),
    )
    .await
    .unwrap()
    .unwrap();
    let on: bool = db::query_scalar("SELECT reported_on FROM devices WHERE id=?")
        .bind(&id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(on);
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn cloud_configuration_serves_journal_without_hardware_workers_or_commands() {
    let mut config: Config = toml::from_str(include_str!("../deploy/config.cloud.toml")).unwrap();
    assert!(!config.automation_enabled);
    assert_eq!(config.database.backend, "azure_sql");
    assert!(!config.database.migrate);
    assert_eq!(config.bind.to_string(), "0.0.0.0:3000");
    assert_eq!(config.sensor.adapter, "disabled");
    assert_eq!(config.camera.adapter, "disabled");
    let dir = tempfile::tempdir().unwrap();
    config.data_dir = dir.path().into();
    config.database = Default::default(); // No credentials or Azure access in this test.
    let app = App::open(config).await.unwrap();
    assert!(app.sensor.is_none());
    assert!(app.camera.is_none());
    assert!(automation::spawn(app.clone()).is_empty());
    let router = api::router(app.clone());
    let (status, _) = request(&router, "GET", "/healthz", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        &router,
        "POST",
        "/api/v1/plants",
        Some(json!({"name":"Cloud fern"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = request(
        &router,
        "PUT",
        "/api/v1/overrides/outlet",
        Some(json!({"on":true})),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _) = request(&router, "DELETE", "/api/v1/overrides/outlet", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _) = request(
        &router,
        "PUT",
        "/api/v1/schedules/outlet",
        Some(json!({"device_id":"outlet","enabled":true,"start_time":"08:00","end_time":"20:00"})),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM overrides")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn seed_inventory_persists_validates_and_deletes() {
    let (dir, app, router) = setup().await;
    assert_eq!(
        request(&router, "GET", "/seeds", None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        request(&router, "GET", "/api/v1/seeds", None).await.1,
        json!([])
    );
    let mut seed = json!({"name":"  Tomato 🌱  ","variety":"O'Brien's <heirloom>","quantity":12,"unit":"seeds","supplier":"Seed library","purchase_year":2026,"storage_location":"Fridge","notes":"Keep dry"});
    let (status, created) = request(&router, "POST", "/api/v1/seeds", Some(seed.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let path = format!("/api/v1/seeds/{}", created["id"].as_str().unwrap());
    for (field, value) in [
        ("quantity", json!(-1)),
        ("quantity", json!(1.5)),
        ("quantity", json!(1_000_000_001i64)),
        ("unit", json!("bags")),
        ("name", json!("  ")),
        ("purchase_year", json!(1800)),
        ("notes", json!("x".repeat(10001))),
    ] {
        let mut invalid = seed.clone();
        invalid[field] = value;
        assert!(request(&router, "PUT", &path, Some(invalid))
            .await
            .0
            .is_client_error());
    }
    let saved = request(&router, "GET", "/api/v1/seeds", None).await.1;
    assert_eq!(saved[0]["quantity"], 12);
    assert_eq!(saved[0]["name"], "Tomato 🌱");
    assert_eq!(saved[0]["variety"], seed["variety"]);
    seed["quantity"] = json!(0);
    seed["unit"] = json!("packets");
    seed["purchase_year"] = Value::Null;
    assert_eq!(
        request(&router, "PUT", &path, Some(seed.clone())).await.0,
        StatusCode::NO_CONTENT
    );
    drop(router);
    app.pool.close().await;
    drop(app);
    let reopened = App::open(Config {
        data_dir: dir.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    let router = api::router(reopened);
    let saved = request(&router, "GET", "/api/v1/seeds", None).await.1;
    assert_eq!(saved[0]["quantity"], 0);
    assert!(saved[0]["purchase_year"].is_null());
    assert_eq!(
        request(&router, "DELETE", &path, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&router, "DELETE", &path, None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&router, "PUT", &path, Some(seed)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&router, "GET", "/api/v1/seeds", None).await.1,
        json!([])
    );
}
