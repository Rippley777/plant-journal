use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use plant_journal::{api, auth, automation, config::Config, database as db, App};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

struct Client {
    router: Router,
    cookie: String,
    garden: String,
}
impl Client {
    async fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value, String) {
        self.raw(
            method,
            path,
            body.map(|v| v.to_string().into_bytes()).unwrap_or_default(),
            "application/json",
        )
        .await
    }
    async fn raw(
        &self,
        method: &str,
        path: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> (StatusCode, Value, String) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost")
            .header("content-type", content_type);
        if !self.cookie.is_empty() {
            request = request.header("cookie", &self.cookie);
        }
        if !self.garden.is_empty() {
            request = request.header("x-garden-id", &self.garden);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let cookie = response
            .headers()
            .get("set-cookie")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            cookie,
        )
    }
    async fn signup(&mut self, email: &str) {
        let (status, result, cookie) = self
            .call(
                "POST",
                "/api/v1/auth/signup",
                Some(json!({"email":email,"password":"A long testing passphrase"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert!(
            cookie.contains("HttpOnly")
                && cookie.contains("SameSite=Lax")
                && cookie.contains("Secure")
        );
        self.cookie = cookie.split(';').next().unwrap().into();
        self.garden = result["garden_id"].as_str().unwrap().into();
    }
    async fn create(&self, path: &str, input: Value) -> String {
        let (status, value, _) = self.call("POST", path, Some(input)).await;
        assert_eq!(status, StatusCode::CREATED, "{value}");
        value["id"].as_str().unwrap().into()
    }
}
async fn setup() -> (tempfile::TempDir, Arc<App>, Client) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::open(Config {
        data_dir: dir.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    let client = Client {
        router: api::router(app.clone()),
        cookie: String::new(),
        garden: String::new(),
    };
    (dir, app, client)
}
#[tokio::test]
async fn public_accounts_garden_isolation_collaboration_and_revocation() {
    let (_dir, app, mut owner) = setup().await;
    assert_eq!(
        owner.call("GET", "/api/v1/plants", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(owner.call("GET", "/", None).await.0, StatusCode::SEE_OTHER);
    assert_eq!(owner.call("GET", "/healthz", None).await.0, StatusCode::OK);
    assert_eq!(owner.call("GET", "/login", None).await.0, StatusCode::OK);
    assert_eq!(
        owner
            .call(
                "POST",
                "/api/v1/auth/signup",
                Some(
                    json!({"email":"ALLY.RIPPLEY@gmail.com","password":"A long testing passphrase"})
                )
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    auth::set_password(
        &app.pool,
        "ally.rippley@gmail.com",
        "A long owner passphrase".into(),
    )
    .await
    .unwrap();
    let (status, result, cookie) = owner
        .call(
            "POST",
            "/api/v1/auth/login",
            Some(json!({"email":"ally.rippley@gmail.com","password":"A long owner passphrase"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    owner.cookie = cookie.split(';').next().unwrap().into();
    owner.garden = result["garden_id"].as_str().unwrap().into();
    let mut other = Client {
        router: owner.router.clone(),
        cookie: String::new(),
        garden: String::new(),
    };
    other.signup("bob@example.com").await;
    let plant = owner
        .create("/api/v1/plants", json!({"name":"Owner fern"}))
        .await;
    let seed = owner
        .create(
            "/api/v1/seeds",
            json!({"name":"Basil","quantity":3,"unit":"packets"}),
        )
        .await;
    let entry=owner.create("/api/v1/entries",json!({"kind":"note","body":"Private observation","occurred_at":chrono::Utc::now().timestamp(),"plant_ids":[plant]})).await;
    let device = owner
        .create(
            "/api/v1/devices",
            json!({"name":"Lamp","role":"light","adapter":"simulated"}),
        )
        .await;
    let (status, photo, _) = owner
        .raw(
            "POST",
            &format!("/api/v1/photos/upload?plant={plant}"),
            b"GIF89afixture".to_vec(),
            "application/octet-stream",
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{photo}");
    let photo = photo["id"].as_str().unwrap();
    for path in [
        "/plants",
        "/seeds",
        "/entries",
        "/photos",
        "/readings",
        "/calendar?month=2026-09",
    ] {
        let (status, rows, _) = other.call("GET", &format!("/api/v1{path}"), None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {rows}");
        assert_eq!(rows, json!([]), "{path}");
    }
    let (_, summary, _) = other.call("GET", "/api/v1/summary", None).await;
    assert_eq!(summary["active_plants"], 0);
    assert_eq!(summary["health"], json!([]));
    let (_, devices, _) = other.call("GET", "/api/v1/devices", None).await;
    assert_eq!(devices, json!({"devices":[],"schedules":[],"overrides":[]}));
    for (method, path, body) in [
        ("GET", format!("/plants/{plant}"), None),
        (
            "PUT",
            format!("/plants/{plant}"),
            Some(json!({"name":"stolen"})),
        ),
        ("DELETE", format!("/seeds/{seed}"), None),
        ("DELETE", format!("/entries/{entry}"), None),
        ("GET", format!("/photos/{photo}/image"), None),
        ("DELETE", format!("/photos/{photo}"), None),
        (
            "PUT",
            format!("/photos/{photo}"),
            Some(json!({"plant_ids":[]})),
        ),
        (
            "PUT",
            format!("/devices/{device}"),
            Some(json!({"name":"stolen","role":"light","adapter":"simulated"})),
        ),
        (
            "PUT",
            format!("/schedules/{device}"),
            Some(
                json!({"device_id":device,"enabled":true,"start_time":"08:00","end_time":"20:00"}),
            ),
        ),
        (
            "PUT",
            format!("/overrides/{device}"),
            Some(json!({"on":true})),
        ),
        ("DELETE", format!("/overrides/{device}"), None),
    ] {
        let (status, result, _) = other.call(method, &format!("/api/v1{path}"), body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}: {result}");
    }
    assert_eq!(
        other
            .call(
                "POST",
                "/api/v1/entries",
                Some(json!({"kind":"note","body":"bad link","occurred_at":1,"plant_ids":[plant]}))
            )
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        other
            .raw(
                "POST",
                &format!("/api/v1/photos/upload?seed={seed}"),
                b"GIF89afixture".to_vec(),
                "application/octet-stream"
            )
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let own_plant = other
        .create("/api/v1/plants", json!({"name":"Bob fern"}))
        .await;
    let own_device = other
        .create(
            "/api/v1/devices",
            json!({"name":"Bob lamp","role":"light","adapter":"simulated"}),
        )
        .await;
    assert_eq!(
        other
            .call(
                "PUT",
                &format!("/api/v1/overrides/{own_device}"),
                Some(json!({"on":true}))
            )
            .await
            .0,
        StatusCode::ACCEPTED
    );
    automation::reconcile(&app, chrono::Utc::now())
        .await
        .unwrap();
    let checked: Option<i64> = db::query_scalar("SELECT checked_at FROM devices WHERE id=?")
        .bind(&own_device)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(
        checked.is_none(),
        "Local worker must never control a different garden"
    );
    assert_eq!(
        other
            .call(
                "PUT",
                "/api/v1/settings",
                Some(
                    json!({"timezone":"Europe/London","photo_enabled":false,"photo_time":"13:00"})
                )
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        owner.call("GET", "/api/v1/settings", None).await.1["timezone"],
        "America/Chicago"
    );
    let own_garden = other.garden.clone();
    other.garden = owner.garden.clone();
    assert_eq!(
        other.call("GET", "/api/v1/plants", None).await.0,
        StatusCode::FORBIDDEN
    );
    other.garden = own_garden.clone();
    assert_eq!(
        other
            .call(
                "POST",
                &format!("/api/v1/gardens/{}/select", owner.garden),
                Some(json!({}))
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        owner
            .call(
                "POST",
                "/api/v1/members",
                Some(json!({"email":"bob@example.com"}))
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    other.garden = owner.garden.clone();
    assert_eq!(
        other
            .call("GET", &format!("/api/v1/plants/{plant}"), None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        other
            .call(
                "PUT",
                &format!("/api/v1/plants/{plant}"),
                Some(json!({"name":"Shared fern"}))
            )
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        other
            .call(
                "PUT",
                &format!("/api/v1/overrides/{device}"),
                Some(json!({"on":true}))
            )
            .await
            .0,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        other
            .call("GET", &format!("/api/v1/photos/{photo}/image"), None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        other
            .call(
                "POST",
                "/api/v1/members",
                Some(json!({"email":"third@example.com"}))
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        other.call("GET", "/api/v1/members", None).await.0,
        StatusCode::FORBIDDEN
    );
    // A member cannot link their private plant into a shared garden.
    assert_eq!(other.call("POST","/api/v1/entries",Some(json!({"kind":"note","body":"cross garden","occurred_at":1,"plant_ids":[own_plant]}))).await.0,StatusCode::BAD_REQUEST);
    let members = owner.call("GET", "/api/v1/members", None).await.1;
    let member = members
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["email"] == "bob@example.com")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    assert_eq!(
        owner
            .call("DELETE", &format!("/api/v1/members/{member}"), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        other.call("GET", "/api/v1/plants", None).await.0,
        StatusCode::FORBIDDEN
    );
    other.garden = own_garden;
    assert_eq!(
        other
            .call("GET", &format!("/api/v1/photos/{photo}/image"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        other
            .call("GET", &format!("/api/v1/plants/{own_plant}"), None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        other
            .call("POST", "/api/v1/auth/logout", Some(json!({})))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        other.call("GET", "/api/v1/plants", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    // Switching a session's default garden must not redirect another tab's writes.
    let (status, extra, _) = owner
        .call("POST", "/api/v1/gardens", Some(json!({"name":"Balcony"})))
        .await;
    assert_eq!(status, StatusCode::OK);
    let extra_id = extra["id"].as_str().unwrap();
    assert_eq!(
        owner
            .call(
                "POST",
                &format!("/api/v1/gardens/{extra_id}/select"),
                Some(json!({}))
            )
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let tab_plant = owner
        .create("/api/v1/plants", json!({"name":"Original tab plant"}))
        .await;
    let new_tab = Client {
        router: owner.router.clone(),
        cookie: owner.cookie.clone(),
        garden: extra_id.into(),
    };
    assert_eq!(
        new_tab
            .call("GET", &format!("/api/v1/plants/{tab_plant}"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        owner
            .call("GET", &format!("/api/v1/plants/{tab_plant}"), None)
            .await
            .0,
        StatusCode::OK
    );
    auth::set_password(
        &app.pool,
        "ally.rippley@gmail.com",
        "A changed owner passphrase".into(),
    )
    .await
    .unwrap();
    assert_eq!(
        owner.call("GET", "/api/v1/plants", None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn migration_preserves_populated_legacy_journal_and_reserves_owner() {
    let dir = tempfile::tempdir().unwrap();
    let migrations = tempfile::tempdir().unwrap();
    for name in [
        "0001_initial.sql",
        "0002_seed_inventory.sql",
        "0003_seed_photos.sql",
    ] {
        std::fs::copy(
            format!("migrations/sqlite/{name}"),
            migrations.path().join(name),
        )
        .unwrap();
    }
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(dir.path().join("journal.sqlite3"))
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(migrations.path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO plants(id,name,created_at) VALUES('existing','Original fern',1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE settings SET timezone='Europe/London',photo_enabled=1")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let app = App::open(Config {
        data_dir: dir.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    let garden: String = db::query_scalar("SELECT garden_id FROM plants WHERE id='existing'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(garden, auth::LEGACY_GARDEN);
    let email: String = db::query_scalar(
        "SELECT u.email FROM users u JOIN gardens g ON g.owner_id=u.id WHERE g.id=?",
    )
    .bind(&garden)
    .fetch_one(&app.pool)
    .await
    .unwrap();
    assert_eq!(email, "ally.rippley@gmail.com");
    let hash: Option<String> = db::query_scalar("SELECT password_hash FROM users WHERE email=?")
        .bind(email)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(hash.is_none());
    let settings = plant_journal::store::garden_settings(&app.pool, &garden)
        .await
        .unwrap();
    assert_eq!(settings.timezone, "Europe/London");
    assert!(settings.photo_enabled);
}

#[tokio::test]
async fn expired_forged_sessions_and_wrong_passwords_are_rejected() {
    let (_dir, app, mut client) = setup().await;
    client.signup("session@example.com").await;
    let real = client.cookie.clone();
    client.cookie =
        "plant_session=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
    assert_eq!(
        client.call("GET", "/api/v1/plants", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    client.cookie = real;
    db::query("UPDATE sessions SET expires_at=0")
        .execute(&app.pool)
        .await
        .unwrap();
    assert_eq!(
        client.call("GET", "/api/v1/plants", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .call(
                "POST",
                "/api/v1/auth/login",
                Some(json!({"email":"session@example.com","password":"Wrong test password"}))
            )
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, _, _) = client
        .call(
            "POST",
            "/api/v1/auth/login",
            Some(json!({"email":"SESSION@example.com","password":"A long testing passphrase"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn repeated_account_attempts_are_rate_limited() {
    let (_dir, _app, client) = setup().await;
    for _ in 0..10 {
        assert_eq!(
            client
                .call(
                    "POST",
                    "/api/v1/auth/signup",
                    Some(json!({"email":"limited@example.com","password":"short"}))
                )
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        client
            .call(
                "POST",
                "/api/v1/auth/login",
                Some(json!({"email":"limited@example.com","password":"wrong password"}))
            )
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
}
