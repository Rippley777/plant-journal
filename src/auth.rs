//! Accounts and garden membership. Only the server derives request identity.
use crate::{
    database::{self as db, Database},
    store, App,
};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    extract::{Path, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
    Extension, Json,
};
use chrono::Utc;
use rand_core::{OsRng, RngCore};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

pub const LEGACY_GARDEN: &str = "00000000-0000-0000-0000-000000000001";
#[derive(Clone)]
pub struct Identity {
    pub user_id: String,
    pub email: String,
    pub garden_id: String,
    pub token_hash: String,
}
#[derive(Clone)]
pub struct Garden(pub String);
pub struct Error(pub StatusCode, pub &'static str);
impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(error=%e,"Account operation failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Account operation failed",
        )
    }
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
type Result<T> = std::result::Result<T, Error>;
fn bad(s: &'static str) -> Error {
    Error(StatusCode::BAD_REQUEST, s)
}
fn unauthorized() -> Error {
    Error(StatusCode::UNAUTHORIZED, "Please sign in")
}
pub fn email(value: &str) -> Result<String> {
    let value = value.trim().to_lowercase();
    if value.len() > 254
        || value.contains(char::is_whitespace)
        || value.split('@').count() != 2
        || value.starts_with('@')
        || !value
            .split('@')
            .nth(1)
            .is_some_and(|v| v.contains('.') && !v.starts_with('.') && !v.ends_with('.'))
    {
        return Err(bad("Enter a valid email address"));
    }
    Ok(value)
}
async fn password_permit() -> anyhow::Result<tokio::sync::OwnedSemaphorePermit> {
    static WORK: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    Ok(WORK
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .acquire_owned()
        .await?)
}
pub async fn hash_password(password: String) -> anyhow::Result<String> {
    anyhow::ensure!(
        (12..=128).contains(&password.chars().count()),
        "Use a password of 12–128 characters"
    );
    let permit = password_permit().await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!(e.to_string()))
    })
    .await?
}
fn digest(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
fn cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| {
            part.trim()
                .strip_prefix("plant_session=")
                .map(str::to_owned)
        })
}
pub async fn identity(app: &App, headers: &HeaderMap) -> Result<Identity> {
    let token = cookie(headers)
        .filter(|s| s.len() == 64)
        .ok_or_else(unauthorized)?;
    let hash = digest(&token);
    let row:db::Record=db::query_as("SELECT u.id,u.email,s.garden_id FROM sessions s JOIN users u ON u.id=s.user_id JOIN garden_members m ON m.garden_id=s.garden_id AND m.user_id=s.user_id WHERE s.token_hash=? AND s.expires_at>?").bind(&hash).bind(Utc::now().timestamp()).fetch_optional(&app.pool).await?.ok_or_else(unauthorized)?;
    Ok(Identity {
        user_id: row.get("id")?,
        email: row.get("email")?,
        garden_id: row.get("garden_id")?,
        token_hash: hash,
    })
}
pub async fn protect(State(app): State<Arc<App>>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if path == "/healthz"
        || path.starts_with("/assets/")
        || matches!(
            path,
            "/login" | "/signup" | "/api/v1/auth/login" | "/api/v1/auth/signup"
        )
    {
        return next.run(req).await;
    }
    match identity(&app, req.headers()).await {
        Ok(mut user) => {
            // Each tab sends its garden explicitly, avoiding cross-tab writes after a switch.
            if let Some(id) = req
                .headers()
                .get("x-garden-id")
                .and_then(|v| v.to_str().ok())
            {
                let member: i64 = match db::query_scalar(
                    "SELECT COUNT(*) FROM garden_members WHERE garden_id=? AND user_id=?",
                )
                .bind(id)
                .bind(&user.user_id)
                .fetch_one(&app.pool)
                .await
                {
                    Ok(n) => n,
                    Err(e) => return Error::from(e).into_response(),
                };
                if member == 0 {
                    return Error(
                        StatusCode::FORBIDDEN,
                        "You do not have access to this garden",
                    )
                    .into_response();
                }
                user.garden_id = id.to_owned();
            }
            req.extensions_mut().insert(Garden(user.garden_id.clone()));
            req.extensions_mut().insert(user);
            next.run(req).await
        }
        Err(e) if e.0 == StatusCode::UNAUTHORIZED && !path.starts_with("/api/") => {
            Redirect::to("/login").into_response()
        }
        Err(e) => e.into_response(),
    }
}
// Bounded per-process limits cap password work and repeated account attempts.
fn rate_limit(key: &str) -> Result<()> {
    static LIMITS: OnceLock<Mutex<HashMap<String, (Instant, u32)>>> = OnceLock::new();
    let mut limits = LIMITS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    limits.retain(|_, (start, _)| start.elapsed() < Duration::from_secs(300));
    if limits.len() >= 10000 {
        return Err(Error(
            StatusCode::TOO_MANY_REQUESTS,
            "Please try again later",
        ));
    }
    for (key, max) in [("all", 100), (key, 10)] {
        let entry = limits.entry(key.into()).or_insert((Instant::now(), 0));
        if entry.1 >= max {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many attempts. Try again in five minutes",
            ));
        }
        entry.1 += 1;
    }
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    email: String,
    password: String,
}
async fn session(app: &App, user_id: &str, garden: &str) -> Result<Response> {
    let mut random = [0u8; 32];
    OsRng.fill_bytes(&mut random);
    let token = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let now = Utc::now().timestamp();
    db::query("DELETE FROM sessions WHERE expires_at<=?")
        .bind(now)
        .execute(&app.pool)
        .await?;
    db::query("INSERT INTO sessions(token_hash,user_id,garden_id,expires_at) VALUES(?,?,?,?)")
        .bind(digest(&token))
        .bind(user_id)
        .bind(garden)
        .bind(now + 604800)
        .execute(&app.pool)
        .await?;
    let cookie = format!(
        "plant_session={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800{}",
        if app.config.secure_cookies {
            "; Secure"
        } else {
            ""
        }
    );
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(json!({"garden_id":garden})),
    )
        .into_response())
}
pub async fn signup(
    State(app): State<Arc<App>>,
    Json(input): Json<Credentials>,
) -> Result<Response> {
    let email = email(&input.email)?;
    rate_limit(&email)?;
    if !(12..=128).contains(&input.password.chars().count()) {
        return Err(bad("Use a password of 12–128 characters"));
    }
    let exists: i64 = db::query_scalar("SELECT COUNT(*) FROM users WHERE email=?")
        .bind(&email)
        .fetch_one(&app.pool)
        .await?;
    if exists > 0 {
        return Err(bad(
            "This email is unavailable. Sign in or contact the administrator for recovery",
        ));
    }
    let hash = hash_password(input.password).await?;
    let user = store::id();
    let garden = store::id();
    let mut tx = app.pool.begin().await?;
    db::query("INSERT INTO users(id,email,password_hash,created_at) VALUES(?,?,?,?)")
        .bind(&user)
        .bind(&email)
        .bind(hash)
        .bind(Utc::now().timestamp())
        .execute(&mut tx)
        .await?;
    create_garden(&mut tx, &garden, &user, "My garden").await?;
    tx.commit().await?;
    session(&app, &user, &garden).await
}
pub async fn login(
    State(app): State<Arc<App>>,
    Json(input): Json<Credentials>,
) -> Result<Response> {
    let email = email(&input.email)?;
    rate_limit(&email)?;
    if input.password.len() > 512 {
        return Err(unauthorized());
    }
    let row: Option<db::Record> = db::query_as("SELECT id,password_hash FROM users WHERE email=?")
        .bind(email)
        .fetch_optional(&app.pool)
        .await?;
    let hash: Option<String> = row
        .as_ref()
        .map(|r| r.get("password_hash"))
        .transpose()?
        .flatten();
    let permit = password_permit().await?;
    let valid = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let Some(hash) = hash else {
            // Match the expensive work for unknown and reserved accounts.
            let _ = Argon2::default()
                .hash_password(input.password.as_bytes(), &SaltString::generate(&mut OsRng));
            return false;
        };
        PasswordHash::new(&hash).is_ok_and(|h| {
            Argon2::default()
                .verify_password(input.password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .map_err(|e| anyhow::anyhow!(e))?;
    if !valid {
        return Err(Error(
            StatusCode::UNAUTHORIZED,
            "Email or password is incorrect",
        ));
    }
    let user: String = row.unwrap().get("id")?;
    let garden: String =
        db::query_scalar("SELECT garden_id FROM garden_members WHERE user_id=? ORDER BY garden_id")
            .bind(&user)
            .fetch_one(&app.pool)
            .await?;
    session(&app, &user, &garden).await
}
pub async fn logout(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
) -> Result<Response> {
    db::query("DELETE FROM sessions WHERE token_hash=?")
        .bind(user.token_hash)
        .execute(&app.pool)
        .await?;
    Ok((
        [(
            header::SET_COOKIE,
            "plant_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        )],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}
pub async fn me(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
) -> Result<Json<Value>> {
    let rows:Vec<db::Record>=db::query_as("SELECT g.id,g.name,g.owner_id FROM gardens g JOIN garden_members m ON m.garden_id=g.id WHERE m.user_id=? ORDER BY g.name").bind(&user.user_id).fetch_all(&app.pool).await?;
    let gardens=rows.iter().map(|r|Ok(json!({"id":r.get::<String>("id")?,"name":r.get::<String>("name")?,"is_owner":r.get::<String>("owner_id")?==user.user_id}))).collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Json(
        json!({"email":user.email,"garden_id":user.garden_id,"gardens":gardens}),
    ))
}
async fn create_garden(
    tx: &mut db::Transaction,
    id: &str,
    user: &str,
    name: &str,
) -> anyhow::Result<()> {
    db::query("INSERT INTO gardens(id,name,owner_id) VALUES(?,?,?)")
        .bind(id)
        .bind(name)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    db::query("INSERT INTO garden_members(garden_id,user_id) VALUES(?,?)")
        .bind(id)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    db::query("INSERT INTO garden_settings(garden_id) VALUES(?)")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GardenInput {
    name: String,
}
pub async fn new_garden(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
    Json(input): Json<GardenInput>,
) -> Result<Json<Value>> {
    if input.name.trim().is_empty() || input.name.len() > 120 {
        return Err(bad("Garden name must contain 1–120 characters"));
    }
    let id = store::id();
    let mut tx = app.pool.begin().await?;
    create_garden(&mut tx, &id, &user.user_id, input.name.trim()).await?;
    tx.commit().await?;
    Ok(Json(json!({"id":id})))
}
pub async fn select_garden(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let count: i64 =
        db::query_scalar("SELECT COUNT(*) FROM garden_members WHERE garden_id=? AND user_id=?")
            .bind(&id)
            .bind(&user.user_id)
            .fetch_one(&app.pool)
            .await?;
    if count == 0 {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "You do not have access to this garden",
        ));
    }
    db::query("UPDATE sessions SET garden_id=? WHERE token_hash=?")
        .bind(id)
        .bind(user.token_hash)
        .execute(&app.pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn owner(app: &App, user: &Identity) -> Result<()> {
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM gardens WHERE id=? AND owner_id=?")
        .bind(&user.garden_id)
        .bind(&user.user_id)
        .fetch_one(&app.pool)
        .await?;
    if count == 0 {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "Only the garden owner can manage collaborators",
        ));
    }
    Ok(())
}
pub async fn members(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
) -> Result<Json<Value>> {
    owner(&app, &user).await?;
    let rows:Vec<db::Record>=db::query_as("SELECT u.id,u.email FROM users u JOIN garden_members m ON m.user_id=u.id WHERE m.garden_id=? ORDER BY u.email").bind(&user.garden_id).fetch_all(&app.pool).await?;
    Ok(Json(Value::Array(rows.iter().map(|r|Ok(json!({"id":r.get::<String>("id")?,"email":r.get::<String>("email")?,"is_owner":r.get::<String>("id")?==user.user_id}))).collect::<anyhow::Result<Vec<_>>>()?)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberInput {
    email: String,
}
pub async fn add_member(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
    Json(input): Json<MemberInput>,
) -> Result<StatusCode> {
    owner(&app, &user).await?;
    let email = email(&input.email)?;
    let id: String =
        db::query_scalar("SELECT id FROM users WHERE email=? AND password_hash IS NOT NULL")
            .bind(email)
            .fetch_optional(&app.pool)
            .await?
            .ok_or_else(|| bad("Ask this person to create an account first"))?;
    db::query("INSERT OR IGNORE INTO garden_members(garden_id,user_id) VALUES(?,?)").sql_server("INSERT INTO garden_members(garden_id,user_id) SELECT @P1,@P2 WHERE NOT EXISTS(SELECT 1 FROM garden_members WITH (UPDLOCK,HOLDLOCK) WHERE garden_id=@P1 AND user_id=@P2)").bind(user.garden_id).bind(id).execute(&app.pool).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn remove_member(
    State(app): State<Arc<App>>,
    Extension(user): Extension<Identity>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    owner(&app, &user).await?;
    if id == user.user_id {
        return Err(bad("The owner cannot be removed"));
    }
    let mut tx = app.pool.begin().await?;
    db::query("DELETE FROM garden_members WHERE garden_id=? AND user_id=?")
        .bind(&user.garden_id)
        .bind(&id)
        .execute(&mut tx)
        .await?;
    // Revoke sessions currently pointing at a removed garden; other sessions recheck membership.
    db::query("DELETE FROM sessions WHERE garden_id=? AND user_id=?")
        .bind(user.garden_id)
        .bind(id)
        .execute(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
/// Administrator-only local recovery; never exposed through the HTTP API.
pub async fn set_password(
    pool: &Database,
    email_address: &str,
    password: String,
) -> anyhow::Result<()> {
    let email = email(email_address).map_err(|e| anyhow::anyhow!(e.1))?;
    let hash = hash_password(password).await?;
    let mut tx = pool.begin().await?;
    let user: String = db::query_scalar("SELECT id FROM users WHERE email=?")
        .bind(email)
        .fetch_one(&mut tx)
        .await?;
    db::query("UPDATE users SET password_hash=? WHERE id=?")
        .bind(hash)
        .bind(&user)
        .execute(&mut tx)
        .await?;
    db::query("DELETE FROM sessions WHERE user_id=?")
        .bind(user)
        .execute(&mut tx)
        .await?;
    tx.commit().await
}
