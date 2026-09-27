//! Opt-in real-backend contract test. Never uses the production journal.
mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use chrono::Utc;
use http_body_util::BodyExt;
use plant_journal::{
    api, automation,
    config::{Config, DatabaseConfig},
    database as db,
    models::Device,
    App,
};
use serde_json::{json, Value};
use tower::ServiceExt;
async fn request(
    router: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .header("cookie", common::COOKIE)
        .method(method)
        .uri(path)
        .header("host", "localhost");
    if body.is_some() {
        request = request.header("content-type", "application/json");
    }
    let response = router
        .clone()
        .oneshot(
            request
                .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
#[ignore = "Requires PLANT_AZURE_TEST=1 and a dedicated empty Azure SQL database ending in _test"]
async fn azure_sql_end_to_end_contract() {
    assert_eq!(std::env::var("PLANT_AZURE_TEST").as_deref(), Ok("1"));
    let name = std::env::var("AZURE_SQL_DATABASE").expect("Set a dedicated test database");
    assert!(
        name.ends_with("_test"),
        "Refusing a database without the _test suffix"
    );
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        data_dir: dir.path().into(),
        database: DatabaseConfig {
            backend: "azure_sql".into(),
            ..DatabaseConfig::default()
        },
        ..Config::default()
    };
    let app = App::open(config.clone()).await.unwrap();
    // Regression for error 266: boundaries must not execute through sp_executesql.
    let mut transaction = app.pool.begin().await.unwrap();
    let transaction_count: i64 = db::query_scalar("SELECT CAST(@@TRANCOUNT AS bigint)")
        .fetch_one(&mut transaction)
        .await
        .unwrap();
    assert_eq!(transaction_count, 1);
    transaction.commit().await.unwrap();
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM plants")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "Use an empty test database");
    common::session(&app).await;
    let router = api::router(app.clone());
    let (status, cards) = request(&router, "GET", "/api/v1/strains", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cards.as_array().unwrap().len(), 152);
    let parent = cards
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "Blue Dream")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let (status, cross) = request(
        &router,
        "POST",
        "/api/v1/strains",
        Some(json!({"name":"Azure test line","status":"wanted","parent_one_id":parent})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{cross}");
    let strain_id = cross["id"].as_str().unwrap();
    let (status, _) = request(
        &router,
        "PUT",
        &format!("/api/v1/strains/{parent}"),
        Some(json!({"name":"Blue Dream","status":"unowned","parent_one_id":strain_id})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let seed = json!({"name":"Tomato 🌱","variety":"O'Brien","quantity":2,"unit":"packets","purchase_year":2026});
    let (status, created) = request(&router, "POST", "/api/v1/seeds", Some(seed.clone())).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let path = format!("/api/v1/seeds/{}", created["id"].as_str().unwrap());
    let (status, seeds) = request(&router, "GET", "/api/v1/seeds", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(seeds[0]["variety"], "O'Brien");
    assert_eq!(
        request(&router, "PUT", &path, Some(seed)).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&router, "DELETE", &path, None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&router, "DELETE", &path, None).await.0,
        StatusCode::NOT_FOUND
    );
    let (status, plant) = request(
        &router,
        "POST",
        "/api/v1/plants",
        Some(json!({"name":"O'Brien's fern 🌿","strain_id":strain_id})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{plant}");
    let id = plant["id"].as_str().unwrap();
    let (_, cards) = request(&router, "GET", "/api/v1/strains", None).await;
    assert_eq!(
        cards
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == strain_id)
            .unwrap()["status"],
        "collected"
    );

    let (status,entry)=request(&router,"POST","/api/v1/entries",Some(json!({"kind":"watering","body":"Confirmed manually","occurred_at":Utc::now().timestamp(),"plant_ids":[id,id]}))).await;
    assert_eq!(status, StatusCode::CREATED, "{entry}");
    let (status,_)=request(&router,"PUT",&format!("/api/v1/entries/{}",entry["id"].as_str().unwrap()),Some(json!({"kind":"note","body":"Edited entry","occurred_at":Utc::now().timestamp(),"plant_ids":[id]}))).await;
    assert_eq!(status, StatusCode::OK);
    let (status, photo) = request(
        &router,
        "POST",
        "/api/v1/photos/capture",
        Some(json!({"plant_ids":[id,id]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{photo}");
    let photo_id = photo["id"].as_str().unwrap();
    let (status, _) = request(
        &router,
        "PUT",
        &format!("/api/v1/photos/{photo_id}"),
        Some(json!({"plant_ids":[id,id]})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let now = Utc::now();
    let date = "2099-01-01";
    assert!(
        automation::capture(&app, now.timestamp(), vec![id.into()], Some(date))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        automation::capture(&app, now.timestamp(), vec![id.into()], Some(date))
            .await
            .unwrap()
            .is_none()
    );
    automation::sample(&app, now.timestamp()).await.unwrap();
    automation::sample(&app, now.timestamp() + 1).await.unwrap(); // exercises health MERGE update
    let (status, device) = request(
        &router,
        "POST",
        "/api/v1/devices",
        Some(json!({"name":"Test fan","role":"fan","adapter":"simulated"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{device}");
    let device_id = device["id"].as_str().unwrap();
    for on in [false, true] {
        let (status, _) = request(
            &router,
            "PUT",
            &format!("/api/v1/overrides/{device_id}"),
            Some(json!({"on":on})),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }
    automation::reconcile(&app, Utc::now()).await.unwrap();
    let device: Device = db::query_as("SELECT * FROM devices WHERE id=?")
        .bind(device_id)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(device.reported_on, Some(true));
    let (status, _) = request(
        &router,
        "PUT",
        &format!("/api/v1/plants/{id}"),
        Some(json!({"name":"Archived fern","archived":true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for path in [
        "/api/v1/summary",
        "/api/v1/plants",
        "/api/v1/entries",
        "/api/v1/photos",
        "/api/v1/devices",
        "/api/v1/settings",
        "/api/v1/readings",
    ] {
        let (status, value) = request(&router, "GET", path, None).await;
        assert_eq!(status, StatusCode::OK, "{path}: {value}");
    }
    let month = now
        .with_timezone(&chrono_tz::America::Chicago)
        .format("%Y-%m");
    for filter in [
        String::new(),
        format!("&plant={id}"),
        "&kind=environment".into(),
    ] {
        let (status, value) = request(
            &router,
            "GET",
            &format!("/api/v1/calendar?month={month}{filter}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{value}");
    }
    let (status, _) = request(
        &router,
        "DELETE",
        &format!("/api/v1/photos/{photo_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = request(
        &router,
        "DELETE",
        &format!("/api/v1/entries/{}", entry["id"].as_str().unwrap()),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Re-opening applies migrations idempotently and retains the daily claim.
    let reopened = App::open(config).await.unwrap();
    assert!(
        automation::capture(&reopened, now.timestamp(), vec![], Some(date))
            .await
            .unwrap()
            .is_none()
    );
    println!("Azure SQL contract verified; test records remain in {name}");
}
