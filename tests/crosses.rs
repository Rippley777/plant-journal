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
async fn plan_input(r: &Router) -> Value {
    let (_, cards) = request(r, "GET", "/api/v1/strains", None, None).await;
    let id = |name| {
        cards
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap()["id"]
            .clone()
    };
    json!({"name":"Future garden","species":"Cannabis","breeder":"My garden","notes":"An idea to revisit.","parent_one_id":id("Blue Dream"),"parent_two_id":id("Gelato")})
}
async fn create(r: &Router, input: Value) -> String {
    let (status, result) = request(r, "POST", "/api/v1/cross-plans", Some(input), None).await;
    assert_eq!(status, StatusCode::CREATED, "{result}");
    result["id"].as_str().unwrap().into()
}
#[tokio::test]
async fn drafts_persist_and_do_not_create_or_collect_strains() {
    let (_dir, app, r) = setup().await;
    let mut input = plan_input(&r).await;
    let id = create(&r, input.clone()).await;
    let path = format!("/api/v1/cross-plans/{id}");
    assert_eq!(
        request(&r, "POST", "/api/v1/cross-plans", Some(input.clone()), None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    input["name"] = json!("A different name");
    input["notes"] = json!("Updated idea");
    assert_eq!(
        request(&r, "PUT", &path, Some(input.clone()), None).await.0,
        StatusCode::OK
    );
    let (_, cards) = request(&r, "GET", "/api/v1/strains", None, None).await;
    assert_eq!(cards.as_array().unwrap().len(), 152);
    assert!(cards
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["status"] == "unowned"));
    let config = app.config.clone();
    drop(r);
    app.pool.close().await;
    drop(app);
    let app = App::open(config).await.unwrap();
    common::session(&app).await;
    let r = api::router(app);
    let (_, plans) = request(&r, "GET", "/api/v1/cross-plans", None, None).await;
    assert_eq!(plans[0]["notes"], "Updated idea");
    assert!(plans[0]["converted_strain_id"].is_null());
    assert_eq!(
        request(
            &r,
            "DELETE",
            &format!(
                "/api/v1/strains/{}",
                input["parent_one_id"].as_str().unwrap()
            ),
            None,
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    input["parent_two_id"] = json!("missing");
    assert_eq!(
        request(&r, "PUT", &path, Some(input.clone()), None).await.0,
        StatusCode::BAD_REQUEST
    );
    input["parent_two_id"] = input["parent_one_id"].clone();
    assert_eq!(
        request(&r, "PUT", &path, Some(input), None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        request(&r, "DELETE", &path, None, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&r, "GET", "/api/v1/cross-plans", None, None)
            .await
            .1,
        json!([])
    );
}
#[tokio::test]
async fn conversion_is_atomic_and_idempotent_under_concurrent_requests() {
    let (_dir, _app, r) = setup().await;
    let mut input = plan_input(&r).await;
    input["name"] = json!("Blue Dream");
    let id = create(&r, input.clone()).await;
    let path = format!("/api/v1/cross-plans/{id}");
    let convert = format!("{path}/convert");
    assert_eq!(
        request(&r, "POST", &convert, Some(json!({})), None).await.0,
        StatusCode::BAD_REQUEST
    );
    let (_, plans) = request(&r, "GET", "/api/v1/cross-plans", None, None).await;
    assert!(plans[0]["converted_strain_id"].is_null());
    let final_name = json!({"name":"My first cross"});
    let (a, b) = tokio::join!(
        request(&r, "POST", &convert, Some(final_name.clone()), None),
        request(&r, "POST", &convert, Some(final_name), None)
    );
    assert!([StatusCode::CREATED, StatusCode::OK].contains(&a.0));
    assert!([StatusCode::CREATED, StatusCode::OK].contains(&b.0));
    assert_ne!(a.0, b.0);
    assert_eq!(a.1["id"], b.1["id"]);
    let (_, cards) = request(&r, "GET", "/api/v1/strains", None, None).await;
    assert_eq!(cards.as_array().unwrap().len(), 153);
    let card = cards
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == a.1["id"])
        .unwrap();
    for field in [
        "species",
        "breeder",
        "notes",
        "parent_one_id",
        "parent_two_id",
    ] {
        assert_eq!(card[field], input[field]);
    }
    assert_eq!(card["name"], "My first cross");
    assert_eq!(card["status"], "collected");
    let (_, plans) = request(&r, "GET", "/api/v1/cross-plans", None, None).await;
    assert_eq!(plans[0]["converted_strain_id"], card["id"]);
    assert!(plans[0]["converted_at"].is_number());
    assert_eq!(
        request(&r, "PUT", &path, Some(input), None).await.0,
        StatusCode::BAD_REQUEST
    );
    let strain_path = format!("/api/v1/strains/{}", card["id"].as_str().unwrap());
    assert_eq!(
        request(&r, "DELETE", &strain_path, None, None).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&r, "DELETE", &path, None, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&r, "GET", "/api/v1/strains", None, None)
            .await
            .1
            .as_array()
            .unwrap()
            .len(),
        153
    );
    assert_eq!(
        request(&r, "DELETE", &strain_path, None, None).await.0,
        StatusCode::NO_CONTENT
    );
}
#[tokio::test]
async fn gardens_cannot_read_change_or_convert_each_others_plans() {
    let (_dir, app, r) = setup().await;
    let input = plan_input(&r).await;
    let id = create(&r, input.clone()).await;
    let (_, other) = request(
        &r,
        "POST",
        "/api/v1/gardens",
        Some(json!({"name":"Other garden"})),
        None,
    )
    .await;
    let garden = other["id"].as_str().unwrap();
    assert_eq!(
        request(&r, "GET", "/api/v1/cross-plans", None, Some(garden))
            .await
            .1,
        json!([])
    );
    assert_eq!(
        request(
            &r,
            "POST",
            "/api/v1/cross-plans",
            Some(input.clone()),
            Some(garden)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let path = format!("/api/v1/cross-plans/{id}");
    assert_eq!(
        request(&r, "PUT", &path, Some(input.clone()), Some(garden))
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&r, "DELETE", &path, None, Some(garden)).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &r,
            "POST",
            &format!("{path}/convert"),
            Some(json!({})),
            Some(garden)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    // Database constraints also refuse cross-garden parent links.
    assert!(db::query("INSERT INTO cross_plans(id,garden_id,name,name_key,parent_one_id,parent_two_id,created_at,updated_at) VALUES('invalid',?,'invalid','invalid',?,?,1,1)")
        .bind(garden).bind(input["parent_one_id"].as_str().unwrap()).bind(input["parent_two_id"].as_str().unwrap()).execute(&app.pool).await.is_err());
}
