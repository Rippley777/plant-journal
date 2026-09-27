use crate::database as db;
use crate::{automation, models::*, store, App};
use askama::Template;
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

type Result<T> = std::result::Result<T, ApiError>;
pub struct ApiError(StatusCode, String);
impl ApiError {
    fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
    fn missing() -> Self {
        Self(StatusCode::NOT_FOUND, "Record not found".into())
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        tracing::error!(%error,"Request failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The operation failed. Check service logs and device health.".into(),
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}

#[derive(Template)]
#[template(path = "app.html")]
struct Page<'a> {
    page: &'a str,
    title: &'a str,
}
async fn page(Path(page): Path<String>) -> Response {
    render(&page)
}
async fn home() -> Response {
    render("dashboard")
}
fn render(page: &str) -> Response {
    let title = match page {
        "dashboard" => "Overview",
        "plants" => "Your plants",
        "journal" => "Journal",
        "calendar" => "Calendar",
        "photos" => "Photo journal",
        "environment" => "Environment",
        "equipment" => "Equipment",
        "settings" => "Settings",
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    match (Page { page, title }).render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { StatusCode::OK }))
        .route("/", get(home))
        .route("/{page}", get(page))
        .route(
            "/assets/app.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../static/app.css"),
                )
            }),
        )
        .route(
            "/assets/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../static/app.js"),
                )
            }),
        )
         .route("/assets/favicon.svg", get(|| async { ([(header::CONTENT_TYPE,"image/svg+xml")], r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#355a43"/><path d="M32 52V22M32 38C9 39 11 13 11 13S36 12 32 38M32 29C32 10 55 10 55 10S56 31 32 29" fill="none" stroke="#e7ecdf" stroke-width="4"/></svg>"##) }))
        .route("/api/v1/summary", get(summary))
        .route("/api/v1/plants", get(plants).post(create_plant))
        .route("/api/v1/plants/{id}", get(plant).put(update_plant))
        .route("/api/v1/entries", get(entries).post(create_entry))
        .route(
            "/api/v1/entries/{id}",
            axum::routing::put(update_entry).delete(delete_entry),
        )
        .route("/api/v1/calendar", get(calendar))
        .route("/api/v1/photos", get(photos))
        .route("/api/v1/photos/capture", post(capture))
        .route(
            "/api/v1/photos/{id}",
            axum::routing::put(link_photo).delete(delete_photo),
        )
        .route("/api/v1/photos/{id}/image", get(photo_image))
        .route("/api/v1/readings", get(readings))
        .route("/api/v1/devices", get(devices).post(create_device))
        .route("/api/v1/devices/{id}", axum::routing::put(update_device))
        .route("/api/v1/schedules/{id}", axum::routing::put(schedule))
        .route(
            "/api/v1/overrides/{id}",
            axum::routing::put(set_override).delete(resume),
        )
        .route("/api/v1/settings", get(settings).put(update_settings))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(same_origin))
        .layer(TraceLayer::new_for_http())
        .with_state(app)
}
async fn same_origin(req: Request<Body>, next: Next) -> Response {
    if !matches!(req.method().as_str(), "GET" | "HEAD" | "OPTIONS") {
        if req
            .headers()
            .get("sec-fetch-site")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v != "same-origin" && v != "none")
        {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"Cross-origin requests are not allowed"})),
            )
                .into_response();
        }
        if let Some(origin) = req.headers().get(header::ORIGIN) {
            let host = req
                .headers()
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let valid = origin
                .to_str()
                .ok()
                .and_then(|v| v.parse::<axum::http::Uri>().ok())
                .is_some_and(|u| {
                    u.authority().is_some_and(|a| a.as_str() == host)
                        && matches!(u.scheme_str(), Some("http" | "https"))
                });
            if !valid {
                return StatusCode::FORBIDDEN.into_response();
            }
        }
        if matches!(req.method().as_str(), "POST" | "PUT" | "PATCH")
            && req
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_none_or(|v| v.split(';').next() != Some("application/json"))
        {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({"error":"Use application/json"})),
            )
                .into_response();
        }
    }
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response.headers_mut().insert("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'".parse().unwrap());
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
}
fn text(value: &str, label: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max {
        return Err(ApiError::bad(format!(
            "{label} must contain 1–{max} characters"
        )));
    }
    Ok(())
}
fn timestamp(time: i64) -> Result<()> {
    if !(0..=4_102_444_800).contains(&time) {
        return Err(ApiError::bad("Date must be between 1970 and 2100"));
    }
    Ok(())
}
async fn validate_plants(app: &App, ids: &[String]) -> Result<()> {
    if ids.len() > 100 {
        return Err(ApiError::bad("Select at most 100 plants"));
    }
    for id in ids {
        let exists: bool = db::query_scalar(
            "SELECT CASE WHEN EXISTS(SELECT 1 FROM plants WHERE id=?) THEN 1 ELSE 0 END",
        )
        .bind(id)
        .fetch_one(&app.pool)
        .await?;
        if !exists {
            return Err(ApiError::bad("A selected plant does not exist"));
        }
    }
    Ok(())
}
async fn summary(State(app): State<Arc<App>>) -> Result<Json<Value>> {
    let latest:Option<Reading>=db::query_as("SELECT recorded_at,temperature_c,humidity_percent FROM readings ORDER BY recorded_at DESC LIMIT 1").sql_server("SELECT TOP (1) recorded_at,temperature_c,humidity_percent FROM readings ORDER BY recorded_at DESC").fetch_optional(&app.pool).await?;
    let health: Vec<Health> = db::query_as("SELECT * FROM health")
        .fetch_all(&app.pool)
        .await?;
    let counts: (i64,i64,i64)=db::query_as("SELECT (SELECT COUNT(*) FROM plants WHERE archived=0),(SELECT COUNT(*) FROM entries),(SELECT COUNT(*) FROM photos)").fetch_one(&app.pool).await?;
    Ok(Json(
        json!({"database_backend":app.pool.backend(),"active_plants":counts.0,"entries":counts.1,"photos":counts.2,"sensor_stale":automation::reading_stale(latest.as_ref(),Utc::now().timestamp()),"latest_reading":latest,"health":health,"sensor_adapter":app.config.sensor.adapter,"camera_adapter":app.config.camera.adapter,"settings":store::settings(&app.pool).await?}),
    ))
}
async fn plants(State(app): State<Arc<App>>) -> Result<Json<Vec<Plant>>> {
    Ok(Json(
        db::query_as("SELECT * FROM plants ORDER BY archived,name COLLATE NOCASE")
            .sql_server(
                "SELECT * FROM plants ORDER BY archived,name COLLATE Latin1_General_100_CI_AS_SC",
            )
            .fetch_all(&app.pool)
            .await?,
    ))
}
async fn plant(State(app): State<Arc<App>>, Path(id): Path<String>) -> Result<Json<Plant>> {
    Ok(Json(
        db::query_as("SELECT * FROM plants WHERE id=?")
            .bind(id)
            .fetch_optional(&app.pool)
            .await?
            .ok_or_else(ApiError::missing)?,
    ))
}
fn check_plant(input: &PlantInput) -> Result<()> {
    text(&input.name, "Name", 120)?;
    if input.species.len() > 160 || input.notes.len() > 10000 {
        return Err(ApiError::bad("Species or notes are too long"));
    }
    Ok(())
}
async fn create_plant(
    State(app): State<Arc<App>>,
    Json(input): Json<PlantInput>,
) -> Result<(StatusCode, Json<Value>)> {
    check_plant(&input)?;
    let id = store::id();
    let now = Utc::now().timestamp();
    let mut tx = app.pool.begin().await?;
    db::query("INSERT INTO plants(id,name,species,notes,archived,created_at) VALUES(?,?,?,?,?,?)")
        .bind(&id)
        .bind(input.name.trim())
        .bind(&input.species)
        .bind(&input.notes)
        .bind(input.archived)
        .bind(now)
        .execute(&mut tx)
        .await?;
    store::event(
        &mut tx,
        "plant",
        &format!("{} added", input.name.trim()),
        now,
        Some(&id),
        "Plant profile created",
        std::slice::from_ref(&id),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn update_plant(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(input): Json<PlantInput>,
) -> Result<Json<Value>> {
    check_plant(&input)?;
    let previous: Plant = db::query_as("SELECT * FROM plants WHERE id=?")
        .bind(&id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(ApiError::missing)?;
    let mut tx = app.pool.begin().await?;
    db::query("UPDATE plants SET name=?,species=?,notes=?,archived=? WHERE id=?")
        .bind(input.name.trim())
        .bind(&input.species)
        .bind(&input.notes)
        .bind(input.archived)
        .bind(&id)
        .execute(&mut tx)
        .await?;
    if input.archived != previous.archived {
        store::event(
            &mut tx,
            "plant",
            &format!(
                "{} {}",
                input.name,
                if input.archived {
                    "archived"
                } else {
                    "restored"
                }
            ),
            Utc::now().timestamp(),
            Some(&id),
            "History retained",
            std::slice::from_ref(&id),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
#[derive(Deserialize, Default)]
struct Filter {
    plant: Option<String>,
}
async fn entries(
    State(app): State<Arc<App>>,
    Query(filter): Query<Filter>,
) -> Result<Json<Vec<Entry>>> {
    let mut items:Vec<Entry>=db::query_as("SELECT e.* FROM entries e WHERE (? IS NULL OR EXISTS(SELECT 1 FROM entry_plants p WHERE p.entry_id=e.id AND p.plant_id=?)) ORDER BY occurred_at DESC").bind(&filter.plant).bind(&filter.plant).fetch_all(&app.pool).await?;
    let mut links = store::plant_links(
        &app.pool,
        store::LinkKind::Entry,
        &items.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
    )
    .await?;
    for item in &mut items {
        item.plant_ids = links.remove(&item.id).unwrap_or_default();
    }
    Ok(Json(items))
}
async fn save_entry(app: &App, id: &str, input: EntryInput, existing: bool) -> Result<()> {
    if !["note", "watering", "feeding", "pruning", "repotting"].contains(&input.kind.as_str()) {
        return Err(ApiError::bad("Unknown care type"));
    }
    text(&input.body, "Entry", 20000)?;
    timestamp(input.occurred_at)?;
    validate_plants(app, &input.plant_ids).await?;
    if input.plant_ids.is_empty() {
        return Err(ApiError::bad("Select at least one plant"));
    }
    let mut tx = app.pool.begin().await?;
    if existing {
        if db::query("UPDATE entries SET kind=?,body=?,occurred_at=? WHERE id=?")
            .bind(&input.kind)
            .bind(&input.body)
            .bind(input.occurred_at)
            .bind(id)
            .execute(&mut tx)
            .await?
            .rows_affected()
            == 0
        {
            return Err(ApiError::missing());
        }
        db::query("DELETE FROM entry_plants WHERE entry_id=?")
            .bind(id)
            .execute(&mut tx)
            .await?;
        db::query("DELETE FROM events WHERE entity_id=?")
            .bind(id)
            .execute(&mut tx)
            .await?;
    } else {
        db::query("INSERT INTO entries(id,kind,body,occurred_at,created_at) VALUES(?,?,?,?,?)")
            .bind(id)
            .bind(&input.kind)
            .bind(&input.body)
            .bind(input.occurred_at)
            .bind(Utc::now().timestamp())
            .execute(&mut tx)
            .await?;
    }
    for plant in &input.plant_ids {
        db::query("INSERT OR IGNORE INTO entry_plants(entry_id,plant_id) VALUES(?,?)").sql_server("INSERT INTO entry_plants(entry_id,plant_id) SELECT @P1,@P2 WHERE NOT EXISTS(SELECT 1 FROM entry_plants WITH (UPDLOCK,HOLDLOCK) WHERE entry_id=@P1 AND plant_id=@P2)")
            .bind(id)
            .bind(plant)
            .execute(&mut tx)
            .await?;
    }
    store::event(
        &mut tx,
        &input.kind,
        &input.body.chars().take(100).collect::<String>(),
        input.occurred_at,
        Some(id),
        &input.body,
        &input.plant_ids,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
async fn create_entry(
    State(app): State<Arc<App>>,
    Json(input): Json<EntryInput>,
) -> Result<(StatusCode, Json<Value>)> {
    let id = store::id();
    save_entry(&app, &id, input, false).await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn update_entry(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(input): Json<EntryInput>,
) -> Result<Json<Value>> {
    save_entry(&app, &id, input, true).await?;
    Ok(Json(json!({"id":id})))
}
async fn delete_entry(State(app): State<Arc<App>>, Path(id): Path<String>) -> Result<StatusCode> {
    let mut tx = app.pool.begin().await?;
    if db::query("DELETE FROM entries WHERE id=?")
        .bind(&id)
        .execute(&mut tx)
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::missing());
    }
    db::query("DELETE FROM events WHERE entity_id=?")
        .bind(&id)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
struct CalendarQuery {
    month: String,
    plant: Option<String>,
    kind: Option<String>,
}
async fn calendar(
    State(app): State<Arc<App>>,
    Query(q): Query<CalendarQuery>,
) -> Result<Json<Vec<Event>>> {
    let tz: Tz = store::settings(&app.pool)
        .await?
        .timezone
        .parse()
        .map_err(|_| ApiError::bad("Invalid timezone"))?;
    if q.month.len() != 7 || store::month_bounds(&q.month, tz).is_err() {
        return Err(ApiError::bad("Month must use YYYY-MM"));
    }
    Ok(Json(
        store::calendar(&app.pool, &q.month, q.plant.as_deref(), q.kind.as_deref()).await?,
    ))
}
async fn photos(State(app): State<Arc<App>>, Query(q): Query<Filter>) -> Result<Json<Vec<Photo>>> {
    let mut items:Vec<Photo>=db::query_as("SELECT p.* FROM photos p WHERE (? IS NULL OR EXISTS(SELECT 1 FROM photo_plants pp WHERE pp.photo_id=p.id AND pp.plant_id=?)) ORDER BY captured_at DESC").bind(&q.plant).bind(&q.plant).fetch_all(&app.pool).await?;
    let mut links = store::plant_links(
        &app.pool,
        store::LinkKind::Photo,
        &items.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
    )
    .await?;
    for item in &mut items {
        item.plant_ids = links.remove(&item.id).unwrap_or_default();
    }
    Ok(Json(items))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureInput {
    plant_ids: Option<Vec<String>>,
}
async fn capture(
    State(app): State<Arc<App>>,
    Json(input): Json<CaptureInput>,
) -> Result<Json<Value>> {
    if app.camera.is_none() {
        return Err(ApiError::bad(
            "Enable a camera adapter in config.toml first",
        ));
    }
    let plants = match input.plant_ids {
        Some(ids) => ids,
        None => {
            db::query_scalar("SELECT id FROM plants WHERE archived=0")
                .fetch_all(&app.pool)
                .await?
        }
    };
    validate_plants(&app, &plants).await?;
    let id = automation::capture(&app, Utc::now().timestamp(), plants, None)
        .await
        .map_err(|e| {
            ApiError(
                StatusCode::BAD_GATEWAY,
                format!("Photo capture failed: {e}"),
            )
        })?;
    Ok(Json(json!({"id":id})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhotoLinks {
    plant_ids: Vec<String>,
}
async fn link_photo(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(input): Json<PhotoLinks>,
) -> Result<StatusCode> {
    validate_plants(&app, &input.plant_ids).await?;
    let exists: bool = db::query_scalar(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM photos WHERE id=?) THEN 1 ELSE 0 END",
    )
    .bind(&id)
    .fetch_one(&app.pool)
    .await?;
    if !exists {
        return Err(ApiError::missing());
    }
    let mut tx = app.pool.begin().await?;
    db::query("DELETE FROM photo_plants WHERE photo_id=?")
        .bind(&id)
        .execute(&mut tx)
        .await?;
    db::query(
        "DELETE FROM event_plants WHERE event_id IN (SELECT id FROM events WHERE entity_id=?)",
    )
    .bind(&id)
    .execute(&mut tx)
    .await?;
    for plant in &input.plant_ids {
        db::query("INSERT OR IGNORE INTO photo_plants(photo_id,plant_id) VALUES(?,?)").sql_server("INSERT INTO photo_plants(photo_id,plant_id) SELECT @P1,@P2 WHERE NOT EXISTS(SELECT 1 FROM photo_plants WITH (UPDLOCK,HOLDLOCK) WHERE photo_id=@P1 AND plant_id=@P2)")
            .bind(&id)
            .bind(plant)
            .execute(&mut tx)
            .await?;
        db::query("INSERT OR IGNORE INTO event_plants(event_id,plant_id) SELECT id,? FROM events WHERE entity_id=?").sql_server("INSERT INTO event_plants(event_id,plant_id) SELECT e.id,@P1 FROM events e WHERE e.entity_id=@P2 AND NOT EXISTS(SELECT 1 FROM event_plants p WITH (UPDLOCK,HOLDLOCK) WHERE p.event_id=e.id AND p.plant_id=@P1)").bind(plant).bind(&id).execute(&mut tx).await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn photo_image(State(app): State<Arc<App>>, Path(id): Path<String>) -> Result<Response> {
    let filename: String = db::query_scalar("SELECT filename FROM photos WHERE id=?")
        .bind(id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(ApiError::missing)?;
    let data = tokio::fs::read(app.config.data_dir.join("photos").join(&filename))
        .await
        .map_err(anyhow::Error::from)?;
    Ok((
        [(
            header::CONTENT_TYPE,
            if filename.ends_with(".svg") {
                "image/svg+xml"
            } else {
                "image/jpeg"
            },
        )],
        data,
    )
        .into_response())
}
async fn delete_photo(State(app): State<Arc<App>>, Path(id): Path<String>) -> Result<StatusCode> {
    let _lock = app.capture_lock.lock().await;
    let filename: String = db::query_scalar("SELECT filename FROM photos WHERE id=?")
        .bind(&id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(ApiError::missing)?;
    let mut tx = app.pool.begin().await?;
    db::query("DELETE FROM photos WHERE id=?")
        .bind(&id)
        .execute(&mut tx)
        .await?;
    db::query("DELETE FROM events WHERE entity_id=?")
        .bind(&id)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;
    match tokio::fs::remove_file(app.config.data_dir.join("photos").join(filename)).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(anyhow::Error::from(e).into()),
    }
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
struct ReadingQuery {
    from: Option<i64>,
    to: Option<i64>,
}
async fn readings(
    State(app): State<Arc<App>>,
    Query(q): Query<ReadingQuery>,
) -> Result<Json<Vec<Reading>>> {
    let end = q.to.unwrap_or_else(|| Utc::now().timestamp() + 1);
    let start = q.from.unwrap_or(end - 86400);
    if start >= end || end.saturating_sub(start) > 31 * 86400 {
        return Err(ApiError::bad("Choose a reading range of at most 31 days"));
    }
    Ok(Json(db::query_as("SELECT recorded_at,temperature_c,humidity_percent FROM readings WHERE recorded_at>=? AND recorded_at<? ORDER BY recorded_at").bind(start).bind(end).fetch_all(&app.pool).await?))
}
async fn devices(State(app): State<Arc<App>>) -> Result<Json<Value>> {
    let mut devices: Vec<Device> = db::query_as("SELECT * FROM devices ORDER BY name")
        .fetch_all(&app.pool)
        .await?;
    for device in &mut devices {
        if device
            .checked_at
            .is_none_or(|t| Utc::now().timestamp() - t > 60)
        {
            device.reported_on = None;
        }
    }
    let schedules: Vec<Schedule> = db::query_as("SELECT * FROM schedules")
        .fetch_all(&app.pool)
        .await?;
    let overrides: Vec<Override> = db::query_as("SELECT * FROM overrides")
        .fetch_all(&app.pool)
        .await?;
    Ok(Json(
        json!({"devices":devices,"schedules":schedules,"overrides":overrides}),
    ))
}
fn validate_device(input: &DeviceInput) -> Result<()> {
    text(&input.name, "Device name", 120)?;
    if !["light", "fan"].contains(&input.role.as_str())
        || !["simulated", "shelly"].contains(&input.adapter.as_str())
        || !(0..=3).contains(&input.channel)
    {
        return Err(ApiError::bad(
            "Choose a light or fan, a supported adapter, and channel 0–3",
        ));
    }
    if input.adapter == "shelly" {
        let url = reqwest::Url::parse(&input.address)
            .map_err(|_| ApiError::bad("Use an outlet URL such as http://192.168.1.50"))?;
        if url.scheme() != "http"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ApiError::bad(
                "Use the outlet's local HTTP origin, without credentials, path, or query",
            ));
        }
    }
    Ok(())
}

async fn create_device(
    State(app): State<Arc<App>>,
    Json(input): Json<DeviceInput>,
) -> Result<(StatusCode, Json<Value>)> {
    validate_device(&input)?;
    let id = store::id();
    let mut tx = app.pool.begin().await?;
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM devices")
        .fetch_one(&mut tx)
        .await?;
    if count >= 8 {
        return Err(ApiError::bad(
            "This grow space supports up to eight outlets",
        ));
    }
    db::query("INSERT INTO devices(id,name,role,adapter,address,channel) VALUES(?,?,?,?,?,?)")
        .bind(&id)
        .bind(input.name.trim())
        .bind(input.role)
        .bind(input.adapter)
        .bind(input.address.trim_end_matches('/'))
        .bind(input.channel)
        .execute(&mut tx)
        .await?;
    db::query("INSERT INTO schedules(device_id) VALUES(?)")
        .bind(&id)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn update_device(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(input): Json<DeviceInput>,
) -> Result<StatusCode> {
    validate_device(&input)?;
    let _lock = app.control_lock.lock().await;
    let previous: Device = db::query_as("SELECT * FROM devices WHERE id=?")
        .bind(&id)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(ApiError::missing)?;
    let changed = previous.adapter != input.adapter
        || previous.address != input.address.trim_end_matches('/')
        || previous.channel != input.channel;
    let mut tx = app.pool.begin().await?;
    db::query("UPDATE devices SET name=?,role=?,adapter=?,address=?,channel=? WHERE id=?")
        .bind(input.name.trim())
        .bind(input.role)
        .bind(input.adapter)
        .bind(input.address.trim_end_matches('/'))
        .bind(input.channel)
        .bind(&id)
        .execute(&mut tx)
        .await?;
    if changed {
        db::query("UPDATE schedules SET enabled=0 WHERE device_id=?")
            .bind(&id)
            .execute(&mut tx)
            .await?;
        db::query("DELETE FROM overrides WHERE device_id=?")
            .bind(&id)
            .execute(&mut tx)
            .await?;
        db::query("UPDATE devices SET commanded_on=NULL,reported_on=NULL,checked_at=NULL,last_error=NULL WHERE id=?").bind(&id).execute(&mut tx).await?;
    }
    store::event(
        &mut tx,
        "system",
        "Outlet configuration updated",
        Utc::now().timestamp(),
        Some(&id),
        if changed {
            "Connection changed; automation disabled. Previous outlet state is unchanged."
        } else {
            "Name or role updated"
        },
        &[],
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn schedule(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(input): Json<Schedule>,
) -> Result<StatusCode> {
    require_automation(&app)?;
    if input.device_id != id
        || automation::parse_time(&input.start_time).is_err()
        || automation::parse_time(&input.end_time).is_err()
        || input.start_time == input.end_time
    {
        return Err(ApiError::bad(
            "Choose different start and end times in HH:MM format",
        ));
    }
    let _lock = app.control_lock.lock().await;
    if db::query("UPDATE schedules SET enabled=?,start_time=?,end_time=? WHERE device_id=?")
        .bind(input.enabled)
        .bind(input.start_time)
        .bind(input.end_time)
        .bind(&id)
        .execute(&app.pool)
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::missing());
    }
    store::log_event(
        &app.pool,
        "system",
        "Equipment schedule updated",
        Utc::now().timestamp(),
        Some(&id),
        if input.enabled {
            "Schedule enabled"
        } else {
            "Schedule disabled; outlet retains its current state"
        },
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn set_override(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(input): Json<OverrideInput>,
) -> Result<StatusCode> {
    require_automation(&app)?;
    if !(1..=1440).contains(&input.minutes) {
        return Err(ApiError::bad("Override must last 1–1440 minutes"));
    }
    let _lock = app.control_lock.lock().await;
    let exists: bool = db::query_scalar(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM devices WHERE id=?) THEN 1 ELSE 0 END",
    )
    .bind(&id)
    .fetch_one(&app.pool)
    .await?;
    if !exists {
        return Err(ApiError::missing());
    }
    db::query("INSERT INTO overrides(device_id,on_state,expires_at) VALUES(?,?,?) ON CONFLICT(device_id) DO UPDATE SET on_state=excluded.on_state,expires_at=excluded.expires_at").sql_server("MERGE overrides WITH (HOLDLOCK) AS target USING (SELECT @P1 device_id,@P2 on_state,@P3 expires_at) AS src ON target.device_id=src.device_id WHEN MATCHED THEN UPDATE SET on_state=src.on_state,expires_at=src.expires_at WHEN NOT MATCHED THEN INSERT(device_id,on_state,expires_at) VALUES(src.device_id,src.on_state,src.expires_at);").bind(&id).bind(input.on).bind(Utc::now().timestamp()+input.minutes*60).execute(&app.pool).await?;
    store::log_event(
        &app.pool,
        "system",
        "Manual override requested",
        Utc::now().timestamp(),
        Some(&id),
        &format!(
            "{} for {} minutes",
            if input.on { "On" } else { "Off" },
            input.minutes
        ),
    )
    .await?;
    Ok(StatusCode::ACCEPTED)
}
async fn resume(State(app): State<Arc<App>>, Path(id): Path<String>) -> Result<StatusCode> {
    require_automation(&app)?;
    let _lock = app.control_lock.lock().await;
    let exists: bool = db::query_scalar(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM devices WHERE id=?) THEN 1 ELSE 0 END",
    )
    .bind(&id)
    .fetch_one(&app.pool)
    .await?;
    if !exists {
        return Err(ApiError::missing());
    }
    // Expire, don't remove: the controller must turn off if there is no enabled schedule.
    db::query("UPDATE overrides SET expires_at=0 WHERE device_id=?")
        .bind(&id)
        .execute(&app.pool)
        .await?;
    store::log_event(
        &app.pool,
        "system",
        "Manual override ended",
        Utc::now().timestamp(),
        Some(&id),
        "Resume configured schedule; otherwise return to off",
    )
    .await?;
    Ok(StatusCode::ACCEPTED)
}
fn require_automation(app: &App) -> Result<()> {
    if !app.config.automation_enabled {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Equipment control is unavailable on this web-only instance".into(),
        ));
    }
    Ok(())
}

async fn settings(State(app): State<Arc<App>>) -> Result<Json<Settings>> {
    Ok(Json(store::settings(&app.pool).await?))
}
async fn update_settings(
    State(app): State<Arc<App>>,
    Json(input): Json<Settings>,
) -> Result<StatusCode> {
    let tz = input
        .timezone
        .parse::<Tz>()
        .map_err(|_| ApiError::bad("Use an IANA timezone, such as America/Chicago"))?;
    if automation::parse_time(&input.photo_time).is_err() {
        return Err(ApiError::bad("Photo time must use HH:MM"));
    }
    if input.photo_enabled && app.camera.is_none() {
        return Err(ApiError::bad("Camera adapter is disabled"));
    }
    let _lock = app.control_lock.lock().await;
    db::query("UPDATE settings SET timezone=?,photo_enabled=?,photo_time=? WHERE id=1")
        .bind(tz.name())
        .bind(input.photo_enabled)
        .bind(input.photo_time)
        .execute(&app.pool)
        .await?;
    store::log_event(
        &app.pool,
        "system",
        "Settings updated",
        Utc::now().timestamp(),
        None,
        "Timezone and photo schedule saved",
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
