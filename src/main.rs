use plant_journal::{api, automation, config::Config, App};
use tracing_subscriber::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "plant_journal=info,tower_http=info".into())
                // The audited INFO TLS milestones drive phase diagnostics; never enable
                // Tiberius packet/query tracing here.
                .add_directive("tiberius::client::connection=info".parse().unwrap()),
        )
        .with(plant_journal::database::diagnostics::HandshakeLayer)
        .with(tracing_subscriber::fmt::layer())
        .init();
    let config = Config::load()?;
    let bind = config.bind;
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("plant-journal {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args == ["--check-database"] {
        plant_journal::database::check_connection(&config.database).await?;
        println!("Azure SQL DNS, TCP, TLS, SQL login, database selection, and SELECT 1 succeeded");
        return Ok(());
    }
    if args == ["--migrate"] {
        let mut database_config = config.database.clone();
        database_config.migrate = true;
        let database = plant_journal::database::Database::open(
            &database_config,
            &config.data_dir.join("journal.sqlite3"),
        )
        .await?;
        println!("{} schema is ready", database.backend());
        database.close().await;
        return Ok(());
    }
    if args.len() == 2 && args[0] == "--set-password" {
        let password = rpassword::prompt_password("New password (12–128 characters): ")?;
        let confirm = rpassword::prompt_password("Confirm password: ")?;
        anyhow::ensure!(password == confirm, "Passwords did not match");
        let app = App::open(config).await?;
        plant_journal::auth::set_password(&app.pool, &args[1], password).await?;
        println!("Password saved; existing sessions revoked.");
        app.pool.close().await;
        return Ok(());
    }
    if !args.is_empty() {
        anyhow::ensure!(
            args.len() == 2 && args[0] == "--import-sqlite",
            "Usage: plant-journal [--version | --check-database | --migrate | --import-sqlite PATH | --set-password EMAIL]"
        );
        anyhow::ensure!(
            config.database.backend == "azure_sql",
            "--import-sqlite requires database.backend=azure_sql"
        );
        let destination = plant_journal::database::Database::open(
            &config.database,
            &config.data_dir.join("journal.sqlite3"),
        )
        .await?;
        let counts =
            plant_journal::import::sqlite_to_database(std::path::Path::new(&args[1]), &destination)
                .await?;
        for (table, count) in counts {
            println!("{table}: {count} rows copied");
        }
        println!("Import complete. Schedules, overrides, and automatic photos are disabled. Photos remain in the configured local data directory.");
        destination.close().await;
        return Ok(());
    }
    let app = App::open(config).await?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let tasks = automation::spawn(app.clone());
    tracing::info!(%bind, "Plant Journal is ready");
    axum::serve(listener, api::router(app.clone()))
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM handler");
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
            }
            #[cfg(not(unix))]
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    for task in tasks {
        task.abort();
        let _ = task.await;
    }
    app.pool.close().await;
    Ok(())
}
