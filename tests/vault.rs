mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use plant_journal::{api, config::Config, database as db, App};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn setup() -> (tempfile::TempDir, Arc<App>, Router) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::open(Config {
        data_dir: dir.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    common::session(&app).await;
    let router = api::router(app.clone());
    (dir, app, router)
}
async fn request(
    r: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    garden: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("cookie", common::COOKIE)
        .header("content-type", "application/json");
    if let Some(garden) = garden {
        req = req.header("X-Garden-ID", garden);
    }
    let res = r
        .clone()
        .oneshot(
            req.body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn packet() -> Value {
    json!({"name":"Tomato","variety":"Purple","quantity":2,"unit":"packets","breeder":"Community breeder","acquired_on":"2026-09-27","packet_code":"Lot A"})
}
fn attempt() -> Value {
    json!({"started_on":"2026-09-27","seeds_sown":4,"seeds_germinated":null,"notes":"First batch"})
}
async fn create(r: &Router, path: &str, value: Value, garden: Option<&str>) -> String {
    let (status, value) = request(r, "POST", path, Some(value), garden).await;
    assert_eq!(status, StatusCode::CREATED, "{value}");
    value["id"].as_str().unwrap().into()
}
#[tokio::test]
async fn packet_attempt_and_plant_history_survive_restart_and_archiving() {
    let (_dir, app, r) = setup().await;
    let seed = create(&r, "/api/v1/seeds", packet(), None).await;
    let path = format!("/api/v1/seeds/{seed}");
    let attempts = format!("{path}/attempts");
    let id = create(&r, &attempts, attempt(), None).await;
    let attempt_path = format!("{attempts}/{id}");
    let (_, all) = request(&r, "GET", "/api/v1/germination-attempts", None, None).await;
    assert!(all[0]["seeds_germinated"].is_null());
    let mut result = attempt();
    result["seeds_germinated"] = json!(3);
    result["notes"] = json!("Three emerged");
    assert_eq!(
        request(&r, "PUT", &attempt_path, Some(result), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let input = json!({"name":"Seedling","seed_id":seed,"germination_id":id,"archived":true});
    let plant = create(&r, "/api/v1/plants", input.clone(), None).await;
    let plant_path = format!("/api/v1/plants/{plant}");
    assert_eq!(
        request(&r, "DELETE", &path, None, None).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&r, "DELETE", &attempt_path, None, None).await.0,
        StatusCode::BAD_REQUEST
    );
    let config = app.config.clone();
    drop(r);
    app.pool.close().await;
    drop(app);
    let app = App::open(config).await.unwrap();
    common::session(&app).await;
    let r = api::router(app);
    let (_, seeds) = request(&r, "GET", "/api/v1/seeds", None, None).await;
    for field in ["quantity", "unit", "breeder", "acquired_on", "packet_code"] {
        assert_eq!(seeds[0][field], packet()[field]);
    }
    let (_, all) = request(&r, "GET", "/api/v1/germination-attempts", None, None).await;
    assert_eq!(all[0]["seeds_germinated"], 3);
    assert_eq!(all[0]["notes"], "Three emerged");
    let (_, saved) = request(&r, "GET", &plant_path, None, None).await;
    assert_eq!(saved["seed_id"], seed);
    assert_eq!(saved["germination_id"], id);
    assert_eq!(saved["archived"], true);
    // Remove the attempt association while retaining the packet origin.
    let mut changed = input;
    changed["germination_id"] = Value::Null;
    assert_eq!(
        request(&r, "PUT", &plant_path, Some(changed.clone()), None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&r, "DELETE", &attempt_path, None, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&r, "DELETE", &path, None, None).await.0,
        StatusCode::BAD_REQUEST
    );
    changed["seed_id"] = Value::Null;
    assert_eq!(
        request(&r, "PUT", &plant_path, Some(changed), None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        request(&r, "DELETE", &path, None, None).await.0,
        StatusCode::NO_CONTENT
    );
}
#[tokio::test]
async fn vault_rejects_invalid_dates_counts_and_mismatched_origins() {
    let (_dir, app, r) = setup().await;
    for date in ["2026-02-30", "2026-1-01", "yesterday", ""] {
        let mut input = packet();
        input["acquired_on"] = json!(date);
        assert_eq!(
            request(&r, "POST", "/api/v1/seeds", Some(input), None)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    let seed = create(&r, "/api/v1/seeds", packet(), None).await;
    let second = create(&r, "/api/v1/seeds", packet(), None).await;
    let path = format!("/api/v1/seeds/{seed}/attempts");
    for (field, value) in [
        ("started_on", json!("2026-02-29")),
        ("seeds_sown", json!(0)),
        ("seeds_sown", json!(1_000_000_001_i64)),
        ("seeds_germinated", json!(-1)),
        ("seeds_germinated", json!(5)),
    ] {
        let mut input = attempt();
        input[field] = value;
        assert_eq!(
            request(&r, "POST", &path, Some(input), None).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let mut input = attempt();
    input["seeds_germinated"] = json!(0);
    let id = create(&r, &path, input, None).await;
    assert_eq!(
        request(&r, "GET", "/api/v1/germination-attempts", None, None)
            .await
            .1[0]["seeds_germinated"],
        0
    );
    for source in [
        json!({"name":"Invalid","germination_id":id}),
        json!({"name":"Invalid","seed_id":second,"germination_id":id}),
        json!({"name":"Invalid","seed_id":"missing"}),
    ] {
        assert_eq!(
            request(&r, "POST", "/api/v1/plants", Some(source), None)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        request(
            &r,
            "PUT",
            &format!("/api/v1/seeds/{second}/attempts/{id}"),
            Some(attempt()),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(db::query(
        "INSERT INTO plants(id,name,created_at,seed_id,germination_id) VALUES('bad','Bad',1,?,?)"
    )
    .bind(&second)
    .bind(&id)
    .execute(&app.pool)
    .await
    .is_err());
    let plant = create(
        &r,
        "/api/v1/plants",
        json!({"name":"Valid","seed_id":seed,"germination_id":id}),
        None,
    )
    .await;
    assert!(
        db::query("UPDATE germination_attempts SET seed_id=? WHERE id=?")
            .bind(&second)
            .bind(&id)
            .execute(&app.pool)
            .await
            .is_err()
    );
    assert!(db::query("UPDATE plants SET seed_id=NULL WHERE id=?")
        .bind(plant)
        .execute(&app.pool)
        .await
        .is_err());
}
#[tokio::test]
async fn vault_records_and_origins_are_garden_scoped() {
    let (_dir, app, r) = setup().await;
    let seed = create(&r, "/api/v1/seeds", packet(), None).await;
    let path = format!("/api/v1/seeds/{seed}/attempts");
    let id = create(&r, &path, attempt(), None).await;
    let (status, created) = request(
        &r,
        "POST",
        "/api/v1/gardens",
        Some(json!({"name":"Other garden"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let garden = created["id"].as_str().unwrap().to_owned();
    let other = Some(garden.as_str());
    let second = create(&r, "/api/v1/seeds", packet(), other).await;
    assert_eq!(
        request(&r, "GET", "/api/v1/germination-attempts", None, other)
            .await
            .1,
        json!([])
    );
    assert_eq!(
        request(&r, "POST", &path, Some(attempt()), other).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&r, "PUT", &format!("{path}/{id}"), Some(attempt()), other)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&r, "DELETE", &format!("{path}/{id}"), None, other)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&r, "DELETE", &format!("/api/v1/seeds/{seed}"), None, other)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    for source in [
        json!({"name":"Invalid","seed_id":seed}),
        json!({"name":"Invalid","seed_id":second,"germination_id":id}),
    ] {
        assert_eq!(
            request(&r, "POST", "/api/v1/plants", Some(source), other)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert!(db::query(
        "INSERT INTO plants(id,garden_id,name,created_at,seed_id) VALUES('bad',?,'Bad',1,?)"
    )
    .bind(&garden)
    .bind(&seed)
    .execute(&app.pool)
    .await
    .is_err());
    assert!(db::query("INSERT INTO germination_attempts(id,garden_id,seed_id,started_on,seeds_sown,created_at) VALUES('bad',?,?,'2026-09-27',1,1)").bind(&garden).bind(&seed).execute(&app.pool).await.is_err());
}
