use plant_journal::{
    automation,
    config::{Config, DatabaseConfig},
    database::{self as db, Database, Value},
    import,
    models::{Plant, Settings},
    App,
};
use tempfile::TempDir;

async fn fixture() -> (TempDir, std::sync::Arc<App>) {
    let dir = tempfile::tempdir().unwrap();
    let app = App::open(Config {
        data_dir: dir.path().into(),
        ..Config::default()
    })
    .await
    .unwrap();
    (dir, app)
}
// The CLI import destination is schema-only (it has never opened the web app).
// Reproduce that state in this isolated fixture without discarding real journals.
async fn import_destination() -> (TempDir, std::sync::Arc<App>) {
    let (dir, app) = fixture().await;
    db::query("UPDATE strains SET parent_one_id=NULL,parent_two_id=NULL")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("DELETE FROM strains")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("DELETE FROM strain_catalog_imports")
        .execute(&app.pool)
        .await
        .unwrap();
    (dir, app)
}
#[test]
fn sql_server_parameters_preserve_quotes_identifiers_and_comments() {
    let sql="SELECT ?, N'what? O''Brien', [odd?]]name], \"quoted?\" -- ?\n/* ? /* nested ? */ ? */ WHERE id=?";
    assert_eq!(db::parameters(sql),"SELECT @P1, N'what? O''Brien', [odd?]]name], \"quoted?\" -- ?\n/* ? /* nested ? */ ? */ WHERE id=@P2");
    assert_eq!(db::parameters("SELECT @P1"), "SELECT @P1");
}
#[tokio::test]
async fn typed_parameters_and_dropped_transactions_are_safe() {
    let (_dir, app) = fixture().await;
    let name = "O'Brien ? 🌱; DROP TABLE plants; --";
    {
        let mut tx = app.pool.begin().await.unwrap();
        db::query("INSERT INTO plants(id,name,created_at) VALUES(?,?,?)")
            .bind("test")
            .bind(name)
            .bind(1_i64)
            .execute(&mut tx)
            .await
            .unwrap();
        let p: Plant = db::query_as("SELECT * FROM plants WHERE id=?")
            .bind("test")
            .fetch_one(&mut tx)
            .await
            .unwrap();
        assert_eq!(p.name, name);
        assert!(!p.archived);
        // Uncommitted scope exits roll back, including API early-return errors.
    }
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM plants")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let empty: Option<String> = db::query_scalar("SELECT NULL")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(empty, None);
    let boolean: bool = db::query_scalar("SELECT ?")
        .bind(true)
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert!(boolean);
    let missing: Option<Plant> = db::query_as("SELECT * FROM plants WHERE id=?")
        .bind("test")
        .fetch_optional(&app.pool)
        .await
        .unwrap();
    assert!(missing.is_none());
}
async fn seed(app: &App) {
    db::query("INSERT INTO plants(id,name,archived,created_at) VALUES('plant','Fern',1,1000)")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("INSERT INTO entries(id,kind,body,occurred_at,created_at) VALUES('entry','watering','Confirmed care',1001,1002)").execute(&app.pool).await.unwrap();
    db::query("INSERT INTO entry_plants(entry_id,plant_id) VALUES('entry','plant')")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("INSERT INTO devices(id,name,role,adapter) VALUES('light','Grow light','light','simulated')").execute(&app.pool).await.unwrap();
    db::query("INSERT INTO schedules(device_id,enabled) VALUES('light',1)")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("INSERT INTO overrides(device_id,on_state,expires_at) VALUES('light',1,9999999999)")
        .execute(&app.pool)
        .await
        .unwrap();
    db::query("UPDATE garden_settings SET timezone='Europe/London',photo_enabled=1")
        .execute(&app.pool)
        .await
        .unwrap();
    automation::capture(app, 1000, vec!["plant".into()], Some("2026-09-26"))
        .await
        .unwrap();
    automation::sample(app, 1000).await.unwrap();
}
#[tokio::test]
async fn import_preserves_history_links_and_claims_but_disables_automation() {
    let (source_dir, source) = fixture().await;
    seed(&source).await;
    db::query("UPDATE plants SET strain_id=(SELECT id FROM strains WHERE name='Blue Dream') WHERE id='plant'").execute(&source.pool).await.unwrap();
    let (_destination_dir, destination) = import_destination().await;
    let counts = import::sqlite_to_database(
        &source_dir.path().join("journal.sqlite3"),
        &destination.pool,
    )
    .await
    .unwrap();
    assert_eq!(counts["plants"], 1);
    assert_eq!(counts["strains"], 152);
    let lineage: String = db::query_scalar("SELECT p.name FROM strains s JOIN strains p ON p.id=s.parent_one_id WHERE s.name='Blue Dream'").fetch_one(&destination.pool).await.unwrap();
    assert_eq!(lineage, "Blueberry");
    assert_eq!(counts["photo_plants"], 1);
    assert_eq!(counts["entry_plants"], 1);
    assert_eq!(counts["readings"], 1);
    let p: Plant = db::query_as("SELECT * FROM plants")
        .fetch_one(&destination.pool)
        .await
        .unwrap();
    assert!(p.archived);
    let imported_strain: String = db::query_scalar("SELECT name FROM strains WHERE id=?")
        .bind(p.strain_id)
        .fetch_one(&destination.pool)
        .await
        .unwrap();
    assert_eq!(imported_strain, "Blue Dream");
    assert_eq!(p.created_at, 1000);
    let settings: Settings =
        db::query_as("SELECT timezone,photo_enabled,photo_time FROM garden_settings")
            .fetch_one(&destination.pool)
            .await
            .unwrap();
    assert_eq!(settings.timezone, "Europe/London");
    assert!(!settings.photo_enabled);
    let enabled: bool = db::query_scalar("SELECT enabled FROM schedules")
        .fetch_one(&destination.pool)
        .await
        .unwrap();
    assert!(!enabled);
    let overrides: i64 = db::query_scalar("SELECT COUNT(*) FROM overrides")
        .fetch_one(&destination.pool)
        .await
        .unwrap();
    assert_eq!(overrides, 0);
    let claim: String =
        db::query_scalar("SELECT status FROM capture_runs WHERE local_date='2026-09-26'")
            .fetch_one(&destination.pool)
            .await
            .unwrap();
    assert_eq!(claim, "complete");
    let source_setting: bool = db::query_scalar("SELECT photo_enabled FROM garden_settings")
        .fetch_one(&source.pool)
        .await
        .unwrap();
    assert!(source_setting, "Source must remain unchanged");
    assert_eq!(
        std::fs::read_dir(source_dir.path().join("photos"))
            .unwrap()
            .count(),
        1
    );
    assert!(import::sqlite_to_database(
        &source_dir.path().join("journal.sqlite3"),
        &destination.pool
    )
    .await
    .is_err());
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM photos")
        .fetch_one(&destination.pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
#[tokio::test]
async fn failed_import_rolls_back_all_preceding_tables() {
    let (source_dir, source) = fixture().await;
    seed(&source).await;
    let (_destination_dir, destination) = import_destination().await;
    db::query("CREATE TRIGGER reject_import BEFORE INSERT ON entry_plants BEGIN SELECT RAISE(ABORT,'injected import failure'); END;").execute(&destination.pool).await.unwrap();
    assert!(import::sqlite_to_database(
        &source_dir.path().join("journal.sqlite3"),
        &destination.pool
    )
    .await
    .is_err());
    for table in [
        "plants",
        "entries",
        "photos",
        "devices",
        "events",
        "readings",
        "strains",
        "strain_catalog_imports",
    ] {
        let count: i64 = db::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&destination.pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "{table} must roll back");
    }
}
#[tokio::test]
async fn azure_configuration_does_not_fall_back_to_sqlite() {
    let dir = tempfile::tempdir().unwrap();
    let config = DatabaseConfig {
        backend: "unknown".into(),
        ..DatabaseConfig::default()
    };
    assert!(
        Database::open(&config, &dir.path().join("should-not-exist.sqlite3"))
            .await
            .is_err()
    );
    assert!(!dir.path().join("should-not-exist.sqlite3").exists());
    assert!(
        toml::from_str::<Config>("[database]\nbackend='azure_sql'\npassword='never-in-toml'")
            .is_err()
    );
    assert_eq!(Value::from(None::<String>), Value::Null);
}

#[tokio::test]
async fn finalized_image_is_retained_if_database_write_fails() {
    let (dir, app) = fixture().await;
    db::query("CREATE TRIGGER photo_write_failure BEFORE INSERT ON photos BEGIN SELECT RAISE(ABORT,'injected database failure'); END;").execute(&app.pool).await.unwrap();
    assert!(automation::capture(&app, 1000, vec![], None).await.is_err());
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM photos")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let files = std::fs::read_dir(dir.path().join("photos"))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(files.len(), 1);
    assert!(!files[0].file_name().to_string_lossy().ends_with(".part"));
}
#[tokio::test]
async fn associations_are_batched_across_sql_server_parameter_boundaries() {
    let (_dir, app) = fixture().await;
    db::query("INSERT INTO plants(id,name,created_at) VALUES('plant','Fern',1000)")
        .execute(&app.pool)
        .await
        .unwrap();
    let mut ids = Vec::new();
    let mut tx = app.pool.begin().await.unwrap();
    for i in 0..1001 {
        let id = format!("entry-{i}");
        db::query("INSERT INTO entries(id,kind,body,occurred_at,created_at) VALUES(?,'note','Test',1000,1000)").bind(&id).execute(&mut tx).await.unwrap();
        db::query("INSERT INTO entry_plants(entry_id,plant_id) VALUES(?,'plant')")
            .bind(&id)
            .execute(&mut tx)
            .await
            .unwrap();
        ids.push(id);
    }
    tx.commit().await.unwrap();
    let links =
        plant_journal::store::plant_links(&app.pool, plant_journal::store::LinkKind::Entry, &ids)
            .await
            .unwrap();
    assert_eq!(links.len(), 1001);
    assert_eq!(links["entry-1000"], vec!["plant"]);
    assert!(plant_journal::store::plant_links(
        &app.pool,
        plant_journal::store::LinkKind::Entry,
        &[]
    )
    .await
    .unwrap()
    .is_empty());
}

#[test]
fn connectivity_cli_requires_exported_environment_and_never_creates_data() {
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        r#"
        data_dir = "diagnostic-data"
        [database]
        backend = "azure_sql"
        server = "localhost"
        name = "ripsql"
        username_env = "PLANT_DIAGNOSTIC_TEST_USER"
        password_env = "PLANT_DIAGNOSTIC_TEST_PASSWORD"
    "#,
    )
    .unwrap();
    // A file alone is not an exported environment, even if it contains PLANT_CONFIG.
    std::fs::write(dir.path().join(".env"), "PLANT_CONFIG=config.toml\n").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_plant-journal"));
    command
        .current_dir(dir.path())
        .arg("--check-database")
        .env_remove("PLANT_CONFIG")
        .env_remove("AZURE_SQL_SERVER")
        .env_remove("AZURE_SQL_DATABASE")
        .env_remove("PLANT_DIAGNOSTIC_TEST_USER")
        .env_remove("PLANT_DIAGNOSTIC_TEST_PASSWORD");
    let output = command.output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("export PLANT_CONFIG"));
    let output = command.env("PLANT_CONFIG", &config).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("PLANT_DIAGNOSTIC_TEST_USER"));
    let output = command
        .env("PLANT_DIAGNOSTIC_TEST_USER", "fake-user")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("PLANT_DIAGNOSTIC_TEST_PASSWORD"));
    assert!(!dir.path().join("diagnostic-data").exists());
    assert!(!dir.path().join("data").exists());
}

#[tokio::test]
async fn seed_inventory_import_and_legacy_sources() {
    let (source_dir, source) = fixture().await;
    let (_dest_dir, dest) = import_destination().await;
    db::query("INSERT INTO seeds(id,name,variety,quantity,unit,purchase_year,created_at) VALUES('seed','Tomato','Purple',3,'packets',2026,1000)").execute(&source.pool).await.unwrap();
    db::query("INSERT INTO photos(id,filename,captured_at,source) VALUES('photo','photo.png',1000,'upload')").execute(&source.pool).await.unwrap();
    db::query("INSERT INTO photo_seeds(photo_id,seed_id) VALUES('photo','seed')")
        .execute(&source.pool)
        .await
        .unwrap();
    let counts = import::sqlite_to_database(&source_dir.path().join("journal.sqlite3"), &dest.pool)
        .await
        .unwrap();
    assert_eq!(counts["seeds"], 1);
    assert_eq!(counts["photo_seeds"], 1);
    let seed: plant_journal::models::Seed = db::query_as("SELECT * FROM seeds")
        .fetch_one(&dest.pool)
        .await
        .unwrap();
    assert_eq!(seed.quantity, 3);
    assert_eq!(seed.purchase_year, Some(2026));
    assert!(
        import::sqlite_to_database(&source_dir.path().join("journal.sqlite3"), &dest.pool)
            .await
            .is_err()
    );
    db::query("DROP TABLE photo_seeds")
        .execute(&source.pool)
        .await
        .unwrap();
    db::query("DROP TABLE seeds")
        .execute(&source.pool)
        .await
        .unwrap();
    let (_legacy_dest_dir, legacy_dest) = import_destination().await;
    let counts = import::sqlite_to_database(
        &source_dir.path().join("journal.sqlite3"),
        &legacy_dest.pool,
    )
    .await
    .unwrap();
    assert_eq!(counts["seeds"], 0);
    assert_eq!(counts["photo_seeds"], 0);
}

#[tokio::test]
async fn seed_migration_upgrades_existing_sqlite_journal() {
    use sqlx::{
        migrate::Migrator,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    let dir = tempfile::tempdir().unwrap();
    let pool = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(dir.path().join("journal.sqlite3"))
                .create_if_missing(true),
        )
        .await
        .unwrap();
    let migrations = tempfile::tempdir().unwrap();
    std::fs::write(
        migrations.path().join("0001_initial.sql"),
        include_str!("../migrations/sqlite/0001_initial.sql"),
    )
    .unwrap();
    Migrator::new(migrations.path())
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO plants(id,name,created_at) VALUES('existing','Fern',1000)")
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
    let name: String = db::query_scalar("SELECT name FROM plants WHERE id='existing'")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(name, "Fern");
    let count: i64 = db::query_scalar("SELECT COUNT(*) FROM seeds")
        .fetch_one(&app.pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
