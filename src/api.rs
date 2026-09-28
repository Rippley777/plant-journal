use crate::auth::{self, Garden};
use crate::database as db;
use crate::{automation, models::*, store, App};
use askama::Template;
use axum::Extension;
use axum::{
    body::{Body, Bytes},
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
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tower_http::trace::TraceLayer;

pub(crate) type Result<T> = std::result::Result<T, ApiError>;
pub struct ApiError(StatusCode, String);
impl ApiError {
    pub(crate) fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
    pub(crate) fn missing() -> Self {
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
async fn auth_page() -> Html<&'static str> {
    Html(include_str!("../templates/auth.html"))
}
async fn home() -> Response {
    render("dashboard")
}
fn render(page: &str) -> Response {
    let title = match page {
        "dashboard" => "Overview",
        "plants" => "Your plants",
        "seeds" => "Seed inventory",
        "strains" => "Strain collection",
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
        .route("/login", get(auth_page))
        .route("/signup", get(auth_page))
        .route("/assets/auth.js", get(|| async { ([(header::CONTENT_TYPE,"text/javascript; charset=utf-8")],include_str!("../static/auth.js")) }))
        .route("/api/v1/auth/signup", post(auth::signup))
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/logout", post(auth::logout))
        .route("/api/v1/auth/me", get(auth::me))
        .route("/api/v1/gardens", post(auth::new_garden))
        .route("/api/v1/gardens/{id}/select", post(auth::select_garden))
        .route("/api/v1/members", get(auth::members).post(auth::add_member))
        .route("/api/v1/members/{id}", axum::routing::delete(auth::remove_member))
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
        .route(
            "/assets/theme.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../static/theme.js"),
                )
            }),
        )
        .route("/api/v1/strains", get(crate::strains::list).post(crate::strains::create))
        .route("/api/v1/strains/{id}", axum::routing::put(crate::strains::update).delete(crate::strains::delete))
        .route("/api/v1/cross-plans", get(crate::crosses::list).post(crate::crosses::create))
        .route("/api/v1/cross-plans/{id}", axum::routing::put(crate::crosses::update).delete(crate::crosses::delete))
        .route("/api/v1/cross-plans/{id}/convert", post(crate::crosses::convert))
        .route("/assets/strains.js", get(|| async { ([(header::CONTENT_TYPE,"text/javascript; charset=utf-8")],include_str!("../static/strains.js")) }))
        .route("/assets/crosses.js", get(|| async { ([(header::CONTENT_TYPE,"text/javascript; charset=utf-8")],include_str!("../static/crosses.js")) }))
        .route("/api/v1/summary", get(summary))
        .route("/api/v1/seeds", get(seeds).post(create_seed))
        .route("/api/v1/seeds/{id}", axum::routing::put(update_seed).delete(delete_seed))
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
        .route("/api/v1/photos/upload", post(upload_photo).layer(DefaultBodyLimit::max(10 * 1024 * 1024)))
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
        .layer(middleware::from_fn_with_state(app.clone(), auth::protect))
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
        let required_type = if req.uri().path() == "/api/v1/photos/upload" {
            "application/octet-stream"
        } else {
            "application/json"
        };
        if matches!(req.method().as_str(), "POST" | "PUT" | "PATCH")
            && req
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_none_or(|v| v.split(';').next() != Some(required_type))
        {
            return (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(json!({"error":format!("Use {required_type}")})),
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
pub(crate) fn text(value: &str, label: &str, max: usize) -> Result<()> {
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
async fn validate_plants(app: &App, garden: &str, ids: &[String]) -> Result<()> {
    if ids.len() > 100 {
        return Err(ApiError::bad("Select at most 100 plants"));
    }
    for id in ids {
        let exists: bool = db::query_scalar(
            "SELECT CASE WHEN EXISTS(SELECT 1 FROM plants WHERE id=? AND garden_id=?) THEN 1 ELSE 0 END",
        )
        .bind(id)
        .bind(&*garden).fetch_one(&app.pool)
        .await?;
        if !exists {
            return Err(ApiError::bad("A selected plant does not exist"));
        }
    }
    Ok(())
}
async fn summary(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
) -> Result<Json<Value>> {
    let latest:Option<Reading>=db::query_as("SELECT recorded_at,temperature_c,humidity_percent FROM readings WHERE garden_id=? ORDER BY recorded_at DESC LIMIT 1").sql_server("SELECT TOP (1) recorded_at,temperature_c,humidity_percent FROM readings WHERE garden_id=? ORDER BY recorded_at DESC").bind(&*garden).fetch_optional(&app.pool).await?;
    let health: Vec<Health> =
        db::query_as("SELECT * FROM health WHERE ?='00000000-0000-0000-0000-000000000001'")
            .bind(&*garden)
            .fetch_all(&app.pool)
            .await?;
    let counts: (i64,i64,i64)=db::query_as("SELECT (SELECT COUNT(*) FROM plants WHERE archived=0 AND garden_id=?),(SELECT COUNT(*) FROM entries WHERE garden_id=?),(SELECT COUNT(*) FROM photos WHERE garden_id=?)").bind(&*garden).bind(&*garden).bind(&*garden).fetch_one(&app.pool).await?;
    Ok(Json(
        json!({"database_backend":app.pool.backend(),"active_plants":counts.0,"entries":counts.1,"photos":counts.2,"sensor_stale":automation::reading_stale(latest.as_ref(),Utc::now().timestamp()),"latest_reading":latest,"health":health,"hardware_connected":garden==auth::LEGACY_GARDEN && app.config.automation_enabled,"sensor_adapter":if garden==auth::LEGACY_GARDEN {app.config.sensor.adapter.as_str()}else{"disabled"},"camera_adapter":if garden==auth::LEGACY_GARDEN {app.config.camera.adapter.as_str()}else{"disabled"},"settings":store::garden_settings(&app.pool, &garden).await?}),
    ))
}
#[derive(Serialize)]
struct PlantWithCover {
    #[serde(flatten)]
    plant: Plant,
    cover_photo_id: Option<String>,
}
async fn plants(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
) -> Result<Json<Vec<PlantWithCover>>> {
    let plants: Vec<Plant> =
        db::query_as("SELECT * FROM plants WHERE garden_id=? ORDER BY archived,name COLLATE NOCASE")
            .sql_server(
                "SELECT * FROM plants WHERE garden_id=? ORDER BY archived,name COLLATE Latin1_General_100_CI_AS_SC",
            )
            .bind(&*garden).fetch_all(&app.pool)
            .await?;
    let photos: Vec<db::Record> = db::query_as("SELECT pp.plant_id,p.id FROM photo_plants pp JOIN photos p ON p.id=pp.photo_id WHERE p.garden_id=? ORDER BY p.captured_at DESC,p.id DESC")
        .bind(&*garden).fetch_all(&app.pool).await?;
    let mut covers = HashMap::new();
    for photo in photos {
        covers
            .entry(photo.get::<String>("plant_id")?)
            .or_insert(photo.get::<String>("id")?);
    }
    Ok(Json(
        plants
            .into_iter()
            .map(|plant| PlantWithCover {
                cover_photo_id: covers.remove(&plant.id),
                plant,
            })
            .collect(),
    ))
}
async fn plant(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
) -> Result<Json<Plant>> {
    Ok(Json(
        db::query_as("SELECT * FROM plants WHERE id=? AND garden_id=?")
            .bind(id)
            .bind(&*garden)
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
    Extension(Garden(garden)): Extension<Garden>,
    Json(input): Json<PlantInput>,
) -> Result<(StatusCode, Json<Value>)> {
    check_plant(&input)?;
    let id = store::id();
    let now = Utc::now().timestamp();
    let mut tx = app.pool.begin().await?;
    crate::strains::lock_garden(&mut tx, &garden).await?;
    crate::strains::link(&mut tx, &garden, input.strain_id.as_deref(), true).await?;
    db::query("INSERT INTO plants(id,name,species,notes,archived,created_at,garden_id,strain_id) VALUES(?,?,?,?,?,?,?,?)")
        .bind(&id)
        .bind(input.name.trim())
        .bind(&input.species)
        .bind(&input.notes)
        .bind(input.archived)
        .bind(now)
        .bind(&*garden).bind(&input.strain_id).execute(&mut tx)
        .await?;
    store::garden_event(
        &mut tx,
        &garden,
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
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<PlantInput>,
) -> Result<Json<Value>> {
    check_plant(&input)?;
    let previous: Plant = db::query_as("SELECT * FROM plants WHERE id=? AND garden_id=?")
        .bind(&id)
        .bind(&*garden)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(ApiError::missing)?;
    let mut tx = app.pool.begin().await?;
    crate::strains::lock_garden(&mut tx, &garden).await?;
    crate::strains::link(&mut tx, &garden, input.strain_id.as_deref(), true).await?;
    db::query("UPDATE plants SET name=?,species=?,notes=?,archived=?,strain_id=? WHERE id=? AND garden_id=?")
        .bind(input.name.trim())
        .bind(&input.species)
        .bind(&input.notes)
        .bind(input.archived)
        .bind(&input.strain_id)
        .bind(&id)
        .bind(&*garden)
        .execute(&mut tx)
        .await?;
    if input.archived != previous.archived {
        store::garden_event(
            &mut tx,
            &garden,
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
    Extension(Garden(garden)): Extension<Garden>,
    Query(filter): Query<Filter>,
) -> Result<Json<Vec<Entry>>> {
    let mut items:Vec<Entry>=db::query_as("SELECT e.* FROM entries e WHERE (? IS NULL OR EXISTS(SELECT 1 FROM entry_plants p WHERE p.entry_id=e.id AND p.plant_id=?)) AND e.garden_id=? ORDER BY occurred_at DESC").bind(&filter.plant).bind(&filter.plant).bind(&*garden).fetch_all(&app.pool).await?;
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
async fn save_entry(
    app: &App,
    garden: &str,
    id: &str,
    input: EntryInput,
    existing: bool,
) -> Result<()> {
    if !["note", "watering", "feeding", "pruning", "repotting"].contains(&input.kind.as_str()) {
        return Err(ApiError::bad("Unknown care type"));
    }
    text(&input.body, "Entry", 20000)?;
    timestamp(input.occurred_at)?;
    validate_plants(app, garden, &input.plant_ids).await?;
    if input.plant_ids.is_empty() {
        return Err(ApiError::bad("Select at least one plant"));
    }
    let mut tx = app.pool.begin().await?;
    if existing {
        if db::query("UPDATE entries SET kind=?,body=?,occurred_at=? WHERE id=? AND garden_id=?")
            .bind(&input.kind)
            .bind(&input.body)
            .bind(input.occurred_at)
            .bind(id)
            .bind(&*garden)
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
        db::query("DELETE FROM events WHERE entity_id=? AND garden_id=?")
            .bind(id)
            .bind(&*garden)
            .execute(&mut tx)
            .await?;
    } else {
        db::query("INSERT INTO entries(id,kind,body,occurred_at,created_at,garden_id) VALUES(?,?,?,?,?,?)")
            .bind(id)
            .bind(&input.kind)
            .bind(&input.body)
            .bind(input.occurred_at)
            .bind(Utc::now().timestamp())
            .bind(&*garden).execute(&mut tx)
            .await?;
    }
    for plant in &input.plant_ids {
        db::query("INSERT OR IGNORE INTO entry_plants(entry_id,plant_id) VALUES(?,?)").sql_server("INSERT INTO entry_plants(entry_id,plant_id) SELECT @P1,@P2 WHERE NOT EXISTS(SELECT 1 FROM entry_plants WITH (UPDLOCK,HOLDLOCK) WHERE entry_id=@P1 AND plant_id=@P2)")
            .bind(id)
            .bind(plant)
            .execute(&mut tx)
            .await?;
    }
    store::garden_event(
        &mut tx,
        &garden,
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
    Extension(Garden(garden)): Extension<Garden>,
    Json(input): Json<EntryInput>,
) -> Result<(StatusCode, Json<Value>)> {
    let id = store::id();
    save_entry(&app, &garden, &id, input, false).await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn update_entry(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<EntryInput>,
) -> Result<Json<Value>> {
    save_entry(&app, &garden, &id, input, true).await?;
    Ok(Json(json!({"id":id})))
}
async fn delete_entry(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let mut tx = app.pool.begin().await?;
    if db::query("DELETE FROM entries WHERE id=? AND garden_id=?")
        .bind(&id)
        .bind(&*garden)
        .execute(&mut tx)
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::missing());
    }
    db::query("DELETE FROM events WHERE entity_id=? AND garden_id=?")
        .bind(&id)
        .bind(&*garden)
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
    Extension(Garden(garden)): Extension<Garden>,
    Query(q): Query<CalendarQuery>,
) -> Result<Json<Vec<Event>>> {
    let tz: Tz = store::garden_settings(&app.pool, &garden)
        .await?
        .timezone
        .parse()
        .map_err(|_| ApiError::bad("Invalid timezone"))?;
    if q.month.len() != 7 || store::month_bounds(&q.month, tz).is_err() {
        return Err(ApiError::bad("Month must use YYYY-MM"));
    }
    Ok(Json(
        store::garden_calendar(
            &app.pool,
            &garden,
            &q.month,
            q.plant.as_deref(),
            q.kind.as_deref(),
        )
        .await?,
    ))
}
struct SeedPhotoLink {
    photo_id: String,
    seed_id: String,
}
impl db::FromRecord for SeedPhotoLink {
    fn from_record(row: &db::Record) -> anyhow::Result<Self> {
        Ok(Self {
            photo_id: row.get("photo_id")?,
            seed_id: row.get("seed_id")?,
        })
    }
}
#[derive(Deserialize)]
struct PhotoFilter {
    plant: Option<String>,
    seed: Option<String>,
}
async fn photos(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Query(q): Query<PhotoFilter>,
) -> Result<Json<Vec<Photo>>> {
    let mut items:Vec<Photo>=db::query_as("SELECT p.* FROM photos p WHERE (? IS NULL OR EXISTS(SELECT 1 FROM photo_plants pp WHERE pp.photo_id=p.id AND pp.plant_id=?)) AND (? IS NULL OR EXISTS(SELECT 1 FROM photo_seeds ps WHERE ps.photo_id=p.id AND ps.seed_id=?)) AND p.garden_id=? ORDER BY captured_at DESC").bind(&q.plant).bind(&q.plant).bind(&q.seed).bind(&q.seed).bind(&*garden).fetch_all(&app.pool).await?;
    let mut links = store::plant_links(
        &app.pool,
        store::LinkKind::Photo,
        &items.iter().map(|item| item.id.clone()).collect::<Vec<_>>(),
    )
    .await?;
    for item in &mut items {
        item.plant_ids = links.remove(&item.id).unwrap_or_default();
    }
    let seed_links: Vec<SeedPhotoLink> = db::query_as("SELECT photo_id,seed_id FROM photo_seeds WHERE photo_id IN (SELECT id FROM photos WHERE garden_id=?)")
        .bind(&*garden).fetch_all(&app.pool)
        .await?;
    for SeedPhotoLink {
        photo_id: photo,
        seed_id: seed,
    } in seed_links
    {
        if let Some(item) = items.iter_mut().find(|item| item.id == photo) {
            item.seed_ids.push(seed);
        }
    }
    Ok(Json(items))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadTarget {
    plant: Option<String>,
    seed: Option<String>,
}

async fn upload_photo(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Query(target): Query<UploadTarget>,
    data: Bytes,
) -> Result<(StatusCode, Json<Value>)> {
    let (table, column, owner) = match (&target.plant, &target.seed) {
        (Some(id), None) => ("photo_plants", "plant_id", id),
        (None, Some(id)) => ("photo_seeds", "seed_id", id),
        _ => return Err(ApiError::bad("Select exactly one plant or seed")),
    };
    let extension = if data.starts_with(b"\xff\xd8\xff") {
        "jpg"
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        "gif"
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        "webp"
    } else {
        return Err(ApiError::bad(
            "Choose a JPEG, PNG, GIF, or WebP image (up to 10 MB)",
        ));
    };
    let exists: i64 = db::query_scalar(if target.plant.is_some() {
        "SELECT COUNT(*) FROM plants WHERE id=? AND garden_id=?"
    } else {
        "SELECT COUNT(*) FROM seeds WHERE id=? AND garden_id=?"
    })
    .bind(owner)
    .bind(&*garden)
    .fetch_one(&app.pool)
    .await?;
    if exists == 0 {
        return Err(ApiError::missing());
    }
    let _lock = app.capture_lock.lock().await;
    let id = store::id();
    let filename = format!("{id}.{extension}");
    let folder = app.config.data_dir.join("photos");
    let destination = folder.join(&filename);
    let now = Utc::now().timestamp();
    let mut committing = false;
    let result: anyhow::Result<()> = async {
        tokio::fs::create_dir_all(&folder).await?;
        tokio::fs::write(&destination, &data).await?;
        tokio::fs::OpenOptions::new()
            .write(true)
            .open(&destination)
            .await?
            .sync_all()
            .await?;
        let mut tx = app.pool.begin().await?;
        db::query("INSERT INTO photos(id,filename,captured_at,source,garden_id) VALUES(?,?,?,?,?)")
            .bind(&id)
            .bind(&filename)
            .bind(now)
            .bind("upload")
            .bind(&*garden)
            .execute(&mut tx)
            .await?;
        db::query(&format!(
            "INSERT INTO {table}(photo_id,{column}) VALUES(?,?)"
        ))
        .bind(&id)
        .bind(owner)
        .execute(&mut tx)
        .await?;
        let plants: Vec<String> = target.plant.iter().cloned().collect();
        store::garden_event(
            &mut tx,
            &garden,
            "photo",
            "Photo uploaded",
            now,
            Some(&id),
            "upload",
            &plants,
        )
        .await?;
        committing = true;
        tx.commit().await
    }
    .await;
    if let Err(error) = result {
        if !committing {
            let _ = tokio::fs::remove_file(&destination).await;
        } else {
            tracing::warn!(photo_id=%id, "Retained uploaded photo after uncertain commit");
        }
        return Err(error.into());
    }
    Ok((StatusCode::CREATED, Json(json!({"id": id}))))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureInput {
    plant_ids: Option<Vec<String>>,
}
async fn capture(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Json(input): Json<CaptureInput>,
) -> Result<Json<Value>> {
    if garden != auth::LEGACY_GARDEN || !app.config.automation_enabled || app.camera.is_none() {
        return Err(ApiError::bad(
            "Enable a camera adapter in config.toml first",
        ));
    }
    let plants = match input.plant_ids {
        Some(ids) => ids,
        None => {
            db::query_scalar("SELECT id FROM plants WHERE archived=0 AND garden_id=?")
                .bind(&*garden)
                .fetch_all(&app.pool)
                .await?
        }
    };
    validate_plants(&app, &garden, &plants).await?;
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
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<PhotoLinks>,
) -> Result<StatusCode> {
    validate_plants(&app, &garden, &input.plant_ids).await?;
    let exists: bool = db::query_scalar(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM photos WHERE id=? AND garden_id=?) THEN 1 ELSE 0 END",
    )
    .bind(&id)
    .bind(&*garden).fetch_one(&app.pool)
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
async fn photo_image(
    State(app): State<Arc<App>>,
    Extension(user): Extension<auth::Identity>,
    Path(id): Path<String>,
) -> Result<Response> {
    let filename: String = db::query_scalar("SELECT p.filename FROM photos p JOIN garden_members m ON m.garden_id=p.garden_id WHERE p.id=? AND m.user_id=?")
        .bind(id)
        .bind(&user.user_id).fetch_optional(&app.pool)
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
            } else if filename.ends_with(".png") {
                "image/png"
            } else if filename.ends_with(".gif") {
                "image/gif"
            } else if filename.ends_with(".webp") {
                "image/webp"
            } else {
                "image/jpeg"
            },
        )],
        data,
    )
        .into_response())
}
async fn delete_photo(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let _lock = app.capture_lock.lock().await;
    let filename: String =
        db::query_scalar("SELECT filename FROM photos WHERE id=? AND garden_id=?")
            .bind(&id)
            .bind(&*garden)
            .fetch_optional(&app.pool)
            .await?
            .ok_or_else(ApiError::missing)?;
    let mut tx = app.pool.begin().await?;
    db::query("DELETE FROM photos WHERE id=? AND garden_id=?")
        .bind(&id)
        .bind(&*garden)
        .execute(&mut tx)
        .await?;
    db::query("DELETE FROM events WHERE entity_id=? AND garden_id=?")
        .bind(&id)
        .bind(&*garden)
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
    Extension(Garden(garden)): Extension<Garden>,
    Query(q): Query<ReadingQuery>,
) -> Result<Json<Vec<Reading>>> {
    let end = q.to.unwrap_or_else(|| Utc::now().timestamp() + 1);
    let start = q.from.unwrap_or(end - 86400);
    if start >= end || end.saturating_sub(start) > 31 * 86400 {
        return Err(ApiError::bad("Choose a reading range of at most 31 days"));
    }
    Ok(Json(db::query_as("SELECT recorded_at,temperature_c,humidity_percent FROM readings WHERE recorded_at>=? AND recorded_at<? AND garden_id=? ORDER BY recorded_at").bind(start).bind(end).bind(&*garden).fetch_all(&app.pool).await?))
}
async fn devices(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
) -> Result<Json<Value>> {
    let mut devices: Vec<Device> =
        db::query_as("SELECT * FROM devices WHERE garden_id=? ORDER BY name")
            .bind(&*garden)
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
    let schedules: Vec<Schedule> = db::query_as(
        "SELECT * FROM schedules WHERE device_id IN (SELECT id FROM devices WHERE garden_id=?)",
    )
    .bind(&*garden)
    .fetch_all(&app.pool)
    .await?;
    let overrides: Vec<Override> = db::query_as(
        "SELECT * FROM overrides WHERE device_id IN (SELECT id FROM devices WHERE garden_id=?)",
    )
    .bind(&*garden)
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
    Extension(Garden(garden)): Extension<Garden>,
    Json(input): Json<DeviceInput>,
) -> Result<(StatusCode, Json<Value>)> {
    validate_device(&input)?;
    let id = store::id();
    let mut tx = app.pool.begin().await?;
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM devices WHERE garden_id=?")
        .bind(&*garden)
        .fetch_one(&mut tx)
        .await?;
    if count >= 8 {
        return Err(ApiError::bad(
            "This grow space supports up to eight outlets",
        ));
    }
    db::query(
        "INSERT INTO devices(id,name,role,adapter,address,channel,garden_id) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(input.name.trim())
    .bind(input.role)
    .bind(input.adapter)
    .bind(input.address.trim_end_matches('/'))
    .bind(input.channel)
    .bind(&*garden)
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
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<DeviceInput>,
) -> Result<StatusCode> {
    validate_device(&input)?;
    let _lock = app.control_lock.lock().await;
    let previous: Device = db::query_as("SELECT * FROM devices WHERE id=? AND garden_id=?")
        .bind(&id)
        .bind(&*garden)
        .fetch_optional(&app.pool)
        .await?
        .ok_or_else(ApiError::missing)?;
    let changed = previous.adapter != input.adapter
        || previous.address != input.address.trim_end_matches('/')
        || previous.channel != input.channel;
    let mut tx = app.pool.begin().await?;
    db::query(
        "UPDATE devices SET name=?,role=?,adapter=?,address=?,channel=? WHERE id=? AND garden_id=?",
    )
    .bind(input.name.trim())
    .bind(input.role)
    .bind(input.adapter)
    .bind(input.address.trim_end_matches('/'))
    .bind(input.channel)
    .bind(&id)
    .bind(&*garden)
    .execute(&mut tx)
    .await?;
    if changed {
        db::query("UPDATE schedules SET enabled=0 WHERE device_id=? AND device_id IN (SELECT id FROM devices WHERE garden_id=?)")
            .bind(&id)
            .bind(&*garden).execute(&mut tx)
            .await?;
        db::query("DELETE FROM overrides WHERE device_id=?")
            .bind(&id)
            .execute(&mut tx)
            .await?;
        db::query("UPDATE devices SET commanded_on=NULL,reported_on=NULL,checked_at=NULL,last_error=NULL WHERE id=? AND garden_id=?").bind(&id).bind(&*garden).execute(&mut tx).await?;
    }
    store::garden_event(
        &mut tx,
        &garden,
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
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<Schedule>,
) -> Result<StatusCode> {
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
    if db::query("UPDATE schedules SET enabled=?,start_time=?,end_time=? WHERE device_id=? AND device_id IN (SELECT id FROM devices WHERE garden_id=?)")
        .bind(input.enabled)
        .bind(input.start_time)
        .bind(input.end_time)
        .bind(&id)
        .bind(&*garden).execute(&app.pool)
        .await?
        .rows_affected()
        == 0
    {
        return Err(ApiError::missing());
    }
    store::garden_log_event(
        &app.pool,
        &garden,
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
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<OverrideInput>,
) -> Result<StatusCode> {
    if !(1..=1440).contains(&input.minutes) {
        return Err(ApiError::bad("Override must last 1–1440 minutes"));
    }
    let _lock = app.control_lock.lock().await;
    let exists: bool = db::query_scalar(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM devices WHERE id=? AND garden_id=?) THEN 1 ELSE 0 END",
    )
    .bind(&id)
    .bind(&*garden).fetch_one(&app.pool)
    .await?;
    if !exists {
        return Err(ApiError::missing());
    }
    db::query("INSERT INTO overrides(device_id,on_state,expires_at) VALUES(?,?,?) ON CONFLICT(device_id) DO UPDATE SET on_state=excluded.on_state,expires_at=excluded.expires_at").sql_server("MERGE overrides WITH (HOLDLOCK) AS target USING (SELECT @P1 device_id,@P2 on_state,@P3 expires_at) AS src ON target.device_id=src.device_id WHEN MATCHED THEN UPDATE SET on_state=src.on_state,expires_at=src.expires_at WHEN NOT MATCHED THEN INSERT(device_id,on_state,expires_at) VALUES(src.device_id,src.on_state,src.expires_at);").bind(&id).bind(input.on).bind(Utc::now().timestamp()+input.minutes*60).execute(&app.pool).await?;
    store::garden_log_event(
        &app.pool,
        &garden,
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
async fn resume(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let _lock = app.control_lock.lock().await;
    let exists: bool = db::query_scalar(
        "SELECT CASE WHEN EXISTS(SELECT 1 FROM devices WHERE id=? AND garden_id=?) THEN 1 ELSE 0 END",
    )
    .bind(&id)
    .bind(&*garden).fetch_one(&app.pool)
    .await?;
    if !exists {
        return Err(ApiError::missing());
    }
    // Expire, don't remove: the controller must turn off if there is no enabled schedule.
    db::query("UPDATE overrides SET expires_at=0 WHERE device_id=? AND device_id IN (SELECT id FROM devices WHERE garden_id=?)")
        .bind(&id)
        .bind(&*garden).execute(&app.pool)
        .await?;
    store::garden_log_event(
        &app.pool,
        &garden,
        "system",
        "Manual override ended",
        Utc::now().timestamp(),
        Some(&id),
        "Resume configured schedule; otherwise return to off",
    )
    .await?;
    Ok(StatusCode::ACCEPTED)
}

async fn settings(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
) -> Result<Json<Settings>> {
    Ok(Json(store::garden_settings(&app.pool, &garden).await?))
}
async fn update_settings(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Json(input): Json<Settings>,
) -> Result<StatusCode> {
    let tz = input
        .timezone
        .parse::<Tz>()
        .map_err(|_| ApiError::bad("Use an IANA timezone, such as America/Chicago"))?;
    if automation::parse_time(&input.photo_time).is_err() {
        return Err(ApiError::bad("Photo time must use HH:MM"));
    }
    if input.photo_enabled
        && (garden != auth::LEGACY_GARDEN || !app.config.automation_enabled || app.camera.is_none())
    {
        return Err(ApiError::bad("Camera adapter is disabled"));
    }
    let _lock = app.control_lock.lock().await;
    db::query(
        "UPDATE garden_settings SET timezone=?,photo_enabled=?,photo_time=? WHERE garden_id=?",
    )
    .bind(tz.name())
    .bind(input.photo_enabled)
    .bind(input.photo_time)
    .bind(&*garden)
    .execute(&app.pool)
    .await?;
    store::garden_log_event(
        &app.pool,
        &garden,
        "system",
        "Settings updated",
        Utc::now().timestamp(),
        None,
        "Timezone and photo schedule saved",
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn seeds(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
) -> Result<Json<Vec<Seed>>> {
    Ok(Json(
        db::query_as("SELECT * FROM seeds WHERE garden_id=? ORDER BY name COLLATE NOCASE,variety,id")
            .sql_server(
                "SELECT * FROM seeds WHERE garden_id=? ORDER BY name COLLATE Latin1_General_100_CI_AS_SC,variety,id",
            )
            .bind(&*garden).fetch_all(&app.pool)
            .await?,
    ))
}
fn check_seed(input: &SeedInput) -> Result<()> {
    text(&input.name, "Name", 120)?;
    if input.variety.len() > 160
        || input.supplier.len() > 160
        || input.storage_location.len() > 160
        || input.notes.len() > 10000
    {
        return Err(ApiError::bad(
            "Variety, supplier, storage location, or notes are too long",
        ));
    }
    if !(0..=1_000_000_000).contains(&input.quantity) {
        return Err(ApiError::bad(
            "Quantity must be a whole number between 0 and 1000000000",
        ));
    }
    if !matches!(input.unit.as_str(), "seeds" | "packets") {
        return Err(ApiError::bad("Unit must be seeds or packets"));
    }
    if input
        .purchase_year
        .is_some_and(|year| !(1900..=2100).contains(&year))
    {
        return Err(ApiError::bad("Purchase year must be between 1900 and 2100"));
    }
    Ok(())
}
async fn create_seed(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Json(input): Json<SeedInput>,
) -> Result<(StatusCode, Json<Value>)> {
    check_seed(&input)?;
    let id = store::id();
    let mut tx = app.pool.begin().await?;
    crate::strains::lock_garden(&mut tx, &garden).await?;
    crate::strains::link(
        &mut tx,
        &garden,
        input.strain_id.as_deref(),
        input.quantity > 0,
    )
    .await?;
    db::query("INSERT INTO seeds(id,name,variety,quantity,unit,supplier,purchase_year,storage_location,notes,created_at,garden_id,strain_id) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
        .bind(&id).bind(input.name.trim()).bind(&input.variety).bind(input.quantity).bind(&input.unit)
        .bind(&input.supplier).bind(input.purchase_year).bind(&input.storage_location).bind(&input.notes)
        .bind(Utc::now().timestamp()).bind(&*garden).bind(&input.strain_id).execute(&mut tx).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"id":id}))))
}
async fn update_seed(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
    Json(input): Json<SeedInput>,
) -> Result<StatusCode> {
    check_seed(&input)?;
    let mut tx = app.pool.begin().await?;
    crate::strains::lock_garden(&mut tx, &garden).await?;
    crate::strains::link(
        &mut tx,
        &garden,
        input.strain_id.as_deref(),
        input.quantity > 0,
    )
    .await?;
    let result = db::query("UPDATE seeds SET name=?,variety=?,quantity=?,unit=?,supplier=?,purchase_year=?,storage_location=?,notes=?,strain_id=? WHERE id=? AND garden_id=?")
        .bind(input.name.trim()).bind(&input.variety).bind(input.quantity).bind(&input.unit)
        .bind(&input.supplier).bind(input.purchase_year).bind(&input.storage_location).bind(&input.notes)
        .bind(&input.strain_id).bind(&id).bind(&*garden).execute(&mut tx).await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn delete_seed(
    State(app): State<Arc<App>>,
    Extension(Garden(garden)): Extension<Garden>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let result = db::query("DELETE FROM seeds WHERE id=? AND garden_id=?")
        .bind(&id)
        .bind(&*garden)
        .execute(&app.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::missing());
    }
    Ok(StatusCode::NO_CONTENT)
}
