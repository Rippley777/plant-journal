use plant_journal::{api, automation, config::Config, App};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "plant_journal=info,tower_http=info".into()),
        )
        .init();
    let config = Config::load()?;
    let bind = config.bind;
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
