mod common;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use plant_journal::{api, config::Config, database as db, strains, App};
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
    router: &Router,
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
    let res = router
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
async fn list(router: &Router) -> Vec<Value> {
    request(router, "GET", "/api/v1/strains", None, None)
        .await
        .1
        .as_array()
        .unwrap()
        .clone()
}
#[tokio::test]
async fn catalog_upgrade_adds_only_missing_cards_and_preserves_existing_edits() {
    let (_dir, app, _router) = setup().await;
    let expanded: Vec<Value> =
        serde_json::from_str(include_str!("../resources/expanded-strains.json")).unwrap();
    db::query("UPDATE strains SET parent_one_id=NULL,parent_two_id=NULL")
        .execute(&app.pool)
        .await
        .unwrap();
    for card in &expanded {
        db::query("DELETE FROM strains WHERE name=?")
            .bind(card["name"].as_str().unwrap())
            .execute(&app.pool)
            .await
            .unwrap();
    }
    db::query("UPDATE strain_catalog_imports SET catalog_version=1")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("UPDATE strains SET status='wanted',notes='Saved edit' WHERE name='OG Kush'")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("DELETE FROM strains WHERE name='ACDC'")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("INSERT INTO strains(id,garden_id,name,name_key,species,breeder,status,lineage_note,source_url,created_at) VALUES('owned-line',?,?,?,'Cannabis','My breeder','wanted','My lineage','',1)")
        .bind(plant_journal::auth::LEGACY_GARDEN).bind("Diesel Regular").bind("diesel regular")
        .execute(&app.pool).await.unwrap();
    strains::seed_starter_collection(&app.pool).await.unwrap();
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM strains")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 151);
    let edited: db::Record = db::query_as("SELECT status,notes FROM strains WHERE name='OG Kush'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(edited.get::<String>("status").unwrap(), "wanted");
    assert_eq!(edited.get::<String>("notes").unwrap(), "Saved edit");
    let breeder: String =
        db::query_scalar("SELECT breeder FROM strains WHERE name='Diesel Regular'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(breeder, "My breeder");
    let absent: i64 = db::query_scalar("SELECT COUNT(*) FROM strains WHERE name='ACDC'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(absent, 0);
    for name in [
        "GG#4 Original Glue Regular",
        "Special Kush #1",
        "Northern Lights 10 of 10",
        "Girl Scout Cookies Fast Version",
        "Do-Si-Dos (Herbies Seeds)",
        "Godfather OG",
        "CBD Amnesia",
        "Bruce Banner Fast Version",
        "Northern Lights #10",
    ] {
        let status: String = db::query_scalar("SELECT status FROM strains WHERE name=?")
            .bind(name)
            .fetch_one(&app.pool)
            .await
            .unwrap();
        assert_eq!(status, "unowned", "{name}");
    }
    let version: i64 = db::query_scalar("SELECT catalog_version FROM strain_catalog_imports")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(version, 2);
    strains::seed_starter_collection(&app.pool).await.unwrap();
    assert_eq!(
        db::query_scalar::<i64>("SELECT COUNT(*) FROM strains")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        151
    );
}
async fn add(router: &Router, name: &str, parents: Option<(&str, &str)>) -> String {
    let mut body = json!({"name":name,"status":"wanted"});
    if let Some((a, b)) = parents {
        body["parent_one_id"] = json!(a);
        body["parent_two_id"] = json!(b);
    }
    let (status, v) = request(router, "POST", "/api/v1/strains", Some(body), None).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    v["id"].as_str().unwrap().into()
}
#[tokio::test]
async fn pedigrees_validate_cycles_duplicates_and_protect_references() {
    let (_dir, _app, r) = setup().await;
    let a = add(&r, "Parent A", None).await;
    let b = add(&r, "Parent B", None).await;
    let child = add(&r, "A × B", Some((&a, &b))).await;
    let grandchild = add(&r, "Next generation", Some((&child, &a))).await;
    let (status, _) = request(
        &r,
        "PUT",
        &format!("/api/v1/strains/{a}"),
        Some(json!({"name":"Parent A","status":"wanted","parent_one_id":grandchild})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        list(&r).await.iter().find(|s| s["id"] == a).unwrap()["parent_one_id"],
        Value::Null
    );
    for parent in [&child, "missing"] {
        assert_eq!(
            request(
                &r,
                "PUT",
                &format!("/api/v1/strains/{child}"),
                Some(json!({"name":"A × B","status":"wanted","parent_one_id":parent})),
                None
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        request(
            &r,
            "POST",
            "/api/v1/strains",
            Some(json!({"name":" parent   a ","status":"wanted"})),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &r,
            "POST",
            "/api/v1/strains",
            Some(json!({"name":"Bad URL","status":"wanted","source_url":"javascript:alert(1)"})),
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&r, "DELETE", &format!("/api/v1/strains/{a}"), None, None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &r,
            "DELETE",
            &format!("/api/v1/strains/{grandchild}"),
            None,
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    // Same-parent ancestry is valid; repeated ancestors are not necessarily a cycle.
    add(&r, "Same-parent line", Some((&b, &b))).await;
}
#[tokio::test]
async fn collected_cards_follow_inventory_and_survive_restart() {
    let (dir, app, r) = setup().await;
    let id = add(&r, "Collection test", None).await;
    let seed = json!({"name":"A packet","quantity":0,"unit":"seeds","strain_id":id});
    let (status, v) = request(&r, "POST", "/api/v1/seeds", Some(seed.clone()), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let seed_id = v["id"].as_str().unwrap();
    assert_eq!(
        list(&r).await.iter().find(|s| s["id"] == id).unwrap()["status"],
        "wanted"
    );
    let mut stocked = seed.clone();
    stocked["quantity"] = json!(4);
    assert_eq!(
        request(
            &r,
            "PUT",
            &format!("/api/v1/seeds/{seed_id}"),
            Some(stocked),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        list(&r).await.iter().find(|s| s["id"] == id).unwrap()["status"],
        "collected"
    );
    request(
        &r,
        "PUT",
        &format!("/api/v1/strains/{id}"),
        Some(json!({"name":"Collection test","status":"wanted"})),
        None,
    )
    .await;
    assert_eq!(
        list(&r).await.iter().find(|s| s["id"] == id).unwrap()["status"],
        "collected"
    );
    request(
        &r,
        "PUT",
        &format!("/api/v1/seeds/{seed_id}"),
        Some(seed),
        None,
    )
    .await;
    assert_eq!(
        list(&r).await.iter().find(|s| s["id"] == id).unwrap()["status"],
        "collected"
    );
    let plant_strain = add(&r, "Plant collection test", None).await;
    let (_, v) = request(
        &r,
        "POST",
        "/api/v1/plants",
        Some(json!({"name":"Linked plant","strain_id":plant_strain})),
        None,
    )
    .await;
    let plant_id = v["id"].as_str().unwrap();
    request(
        &r,
        "PUT",
        &format!("/api/v1/plants/{plant_id}"),
        Some(json!({"name":"Linked plant","strain_id":plant_strain,"archived":true})),
        None,
    )
    .await;
    assert_eq!(
        list(&r)
            .await
            .iter()
            .find(|s| s["id"] == plant_strain)
            .unwrap()["status"],
        "collected"
    );
    assert_eq!(
        request(
            &r,
            "DELETE",
            &format!("/api/v1/strains/{plant_strain}"),
            None,
            None
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    // A failed record save must not leave a card unlocked.
    let untouched = add(&r, "No partial unlock", None).await;
    assert_eq!(
        request(
            &r,
            "PUT",
            "/api/v1/seeds/nonexistent",
            Some(json!({"name":"Missing","unit":"seeds","quantity":2,"strain_id":untouched})),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        list(&r)
            .await
            .iter()
            .find(|s| s["id"] == untouched)
            .unwrap()["status"],
        "wanted"
    );
    let config = app.config.clone();
    drop(r);
    app.pool.close().await;
    drop(app);
    let reopened = App::open(config).await.unwrap();
    common::session(&reopened).await;
    let cards = list(&api::router(reopened)).await;
    assert_eq!(cards.len(), 155);
    assert_eq!(
        cards.iter().find(|s| s["id"] == id).unwrap()["status"],
        "collected"
    );
    assert!(dir.path().join("journal.sqlite3").exists());
}
#[tokio::test]
async fn gardens_isolate_strains_parents_and_inventory_links() {
    let (_dir, app, r) = setup().await;
    let foreign = add(&r, "Private strain", None).await;
    let (_, garden) = request(
        &r,
        "POST",
        "/api/v1/gardens",
        Some(json!({"name":"Other garden"})),
        None,
    )
    .await;
    let garden = garden["id"].as_str().unwrap();
    assert_eq!(
        request(&r, "GET", "/api/v1/strains", None, Some(garden))
            .await
            .1,
        json!([])
    );
    assert_eq!(
        request(
            &r,
            "PUT",
            &format!("/api/v1/strains/{foreign}"),
            Some(json!({"name":"Private strain","status":"collected"})),
            Some(garden)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &r,
            "DELETE",
            &format!("/api/v1/strains/{foreign}"),
            None,
            Some(garden)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &r,
            "POST",
            "/api/v1/strains",
            Some(json!({"name":"Foreign ancestry","status":"unowned","parent_one_id":foreign})),
            Some(garden)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &r,
            "POST",
            "/api/v1/plants",
            Some(json!({"name":"Plant","strain_id":foreign})),
            Some(garden)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &r,
            "POST",
            "/api/v1/seeds",
            Some(json!({"name":"Seeds","unit":"seeds","quantity":2,"strain_id":foreign})),
            Some(garden)
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(db::query("INSERT INTO plants(id,garden_id,name,created_at,strain_id) VALUES('bad-link',?,'Test',1,?)").bind(garden).bind(&foreign).execute(&app.pool).await.is_err());
    assert_eq!(
        list(&r).await.iter().find(|s| s["id"] == foreign).unwrap()["status"],
        "wanted"
    );
}
#[tokio::test]
async fn concurrent_parent_edits_cannot_create_cycle() {
    let (_dir, _app, r) = setup().await;
    let a = add(&r, "Concurrent A", None).await;
    let b = add(&r, "Concurrent B", None).await;
    let pa = format!("/api/v1/strains/{a}");
    let pb = format!("/api/v1/strains/{b}");
    let (one, two) = tokio::join!(
        request(
            &r,
            "PUT",
            &pa,
            Some(json!({"name":"Concurrent A","status":"unowned","parent_one_id":b})),
            None
        ),
        request(
            &r,
            "PUT",
            &pb,
            Some(json!({"name":"Concurrent B","status":"unowned","parent_one_id":a})),
            None
        )
    );
    assert!(
        (one.0 == StatusCode::OK && two.0 == StatusCode::BAD_REQUEST)
            || (two.0 == StatusCode::OK && one.0 == StatusCode::BAD_REQUEST)
    );
}
#[tokio::test]
async fn upgrade_seeds_only_original_garden_and_matches_existing_inventory_once() {
    let dir = tempfile::tempdir().unwrap();
    let migrations = tempfile::tempdir().unwrap();
    for name in [
        "0001_initial.sql",
        "0002_seed_inventory.sql",
        "0003_seed_photos.sql",
        "0004_gardens.sql",
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
    sqlx::query("INSERT INTO plants(id,name,species,created_at) VALUES('old','My plant',' blue dream ',1),('other','Monstera','',2)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO seeds(id,name,variety,quantity,unit,created_at) VALUES('empty','Seeds','GSC',0,'seeds',1),('full','Gelato','',3,'seeds',1)").execute(&pool).await.unwrap();
    pool.close().await;
    let import_dir = tempfile::tempdir().unwrap();
    let destination = db::Database::open(
        &plant_journal::config::DatabaseConfig::default(),
        &import_dir.path().join("journal.sqlite3"),
    )
    .await
    .unwrap();
    let counts = plant_journal::import::sqlite_to_database(
        &dir.path().join("journal.sqlite3"),
        &destination,
    )
    .await
    .unwrap();
    assert_eq!(counts["strains"], 0);
    assert_eq!(counts["plants"], 2);
    assert_eq!(counts["seeds"], 2);
    let legacy_link: Option<String> =
        db::query_scalar("SELECT strain_id FROM plants WHERE id='old'")
            .fetch_one(&destination)
            .await
            .unwrap();
    assert!(legacy_link.is_none());
    destination.close().await;
    let app = App::open(Config {
        data_dir: dir.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM strains")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 152);
    let collected: Vec<db::Record> =
        db::query_as("SELECT name FROM strains WHERE status='collected' ORDER BY name")
            .fetch_all(&app.pool)
            .await
            .unwrap();
    assert_eq!(
        collected
            .iter()
            .map(|r| r.get::<String>("name").unwrap())
            .collect::<Vec<_>>(),
        vec!["Blue Dream", "Gelato"]
    );
    let empty_link: Option<String> =
        db::query_scalar("SELECT strain_id FROM seeds WHERE id='empty'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(empty_link.is_some());
    let unrelated: Option<String> =
        db::query_scalar("SELECT strain_id FROM plants WHERE id='other'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert!(unrelated.is_none());
    db::query("UPDATE strains SET notes='My custom notes',status='wanted' WHERE name='OG Kush'")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("DELETE FROM strains WHERE name='ACDC'")
        .execute(&app.pool)
        .await
        .unwrap();
    strains::seed_starter_collection(&app.pool).await.unwrap();
    assert_eq!(
        db::query_scalar::<i64>("SELECT COUNT(*) FROM strains")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        151
    );
    assert_eq!(
        db::query_scalar::<String>("SELECT notes FROM strains WHERE name='OG Kush'")
            .fetch_one(&app.pool)
            .await
            .unwrap(),
        "My custom notes"
    );
}
