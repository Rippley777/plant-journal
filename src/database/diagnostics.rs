//! Safe metadata for diagnostics. Never log SQL, bound values, or driver error text:
//! SQL Server can echo supplied data in error messages. Keep the original error as a source.
use std::{
    cell::Cell,
    future::Future,
    sync::{Arc, Mutex},
};
use tokio::time::Instant;
use tracing_subscriber::Layer;

// Scoped subscribers rebuild tracing's global callsite interest cache. Serialize
// tests that install them so parallel tests cannot interfere with log capture.
#[cfg(test)]
pub(super) static TEST_TRACING_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

tokio::task_local! {
    static HANDSHAKE_PHASE: Cell<&'static str>;
}

/// Tiberius 0.12.3 exposes TLS milestones as events, not separate public handshake APIs.
/// Observe only these two constant messages, within this task's connection attempt.
/// No packets, credentials, or server-supplied messages are inspected.
pub struct HandshakeLayer;
impl<S: tracing::Subscriber> Layer<S> for HandshakeLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() != "tiberius::client::connection" {
            return;
        }
        #[derive(Default)]
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message::default();
        event.record(&mut message);
        let next = match message.0.as_str() {
            "Performing a TLS handshake" => "tls",
            "TLS handshake successful" => "login",
            _ => return,
        };
        let _ = HANDSHAKE_PHASE.try_with(|phase| phase.set(next));
    }
}

pub(super) fn classify_sql_code(code: u32) -> &'static str {
    match code {
        18456 | 18452 => "authentication",
        4060 | 916 => "database_selection",
        // 40615 is an explicit Azure firewall rejection, not a generic timeout.
        40615 => "server_firewall",
        _ => "sql_server",
    }
}

pub(super) fn failure(stage: &'static str, error: anyhow::Error) -> anyhow::Error {
    let driver = error.downcast_ref::<tiberius::error::Error>();
    let sql_code = driver.and_then(|e| e.code());
    let sql_state = match driver {
        Some(tiberius::error::Error::Server(e)) => Some(e.state()),
        _ => None,
    };
    let category = match driver {
        Some(tiberius::error::Error::Tls(_)) => "tls",
        Some(tiberius::error::Error::Server(e)) => classify_sql_code(e.code()),
        Some(tiberius::error::Error::Protocol(_)) => "tds_protocol",
        _ => stage,
    };
    let timed_out = error
        .downcast_ref::<tokio::time::error::Elapsed>()
        .is_some();
    let io_kind = error
        .downcast_ref::<std::io::Error>()
        .map(|e| e.kind())
        .or(match driver {
            Some(tiberius::error::Error::Io { kind, .. }) => Some(*kind),
            _ => None,
        });
    tracing::error!(
        stage,
        category,
        timed_out,
        sql_code,
        sql_state,
        ?io_kind,
        source_depth = error.chain().count(),
        "Azure SQL operation failed"
    );
    let status = if timed_out { "timed out" } else { "failed" };
    error.context(format!(
        "Azure SQL {stage} {status} (category={category}, sql_code={sql_code:?})"
    ))
}

pub(super) async fn operation<T>(
    stage: &'static str,
    deadline: Instant,
    future: impl Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    let query = matches!(stage, "query" | "pool_validation");
    if !query {
        tracing::info!(stage, "Azure SQL operation started");
    }
    let result = tokio::time::timeout_at(deadline, future).await;
    let result = result
        .map_err(anyhow::Error::from)
        .and_then(|r| r)
        .map_err(|e| failure(stage, e));
    if result.is_ok() && !query {
        tracing::info!(stage, "Azure SQL operation completed");
    }
    result
}

pub(super) async fn handshake<T>(
    deadline: Instant,
    future: impl Future<Output = Result<T, tiberius::error::Error>>,
) -> anyhow::Result<T> {
    HANDSHAKE_PHASE
        .scope(Cell::new("tds_prelogin_or_handshake"), async {
            let result = tokio::time::timeout_at(deadline, future)
                .await
                .map_err(anyhow::Error::from)
                .and_then(|r| r.map_err(anyhow::Error::from));
            result.map_err(|error| {
                // Routing is a normal login response; the caller handles it, keeping the deadline.
                if matches!(
                    error.downcast_ref(),
                    Some(tiberius::error::Error::Routing { .. })
                ) {
                    error
                } else {
                    failure(HANDSHAKE_PHASE.with(Cell::get), error)
                }
            })
        })
        .await
}

#[derive(Clone, Default)]
pub(super) struct PoolErrorSink(Arc<Mutex<Option<(Instant, SharedFailure)>>>);
impl std::fmt::Debug for PoolErrorSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PoolErrorSink")
    }
}
#[derive(Clone)]
pub(super) struct SharedFailure(Arc<anyhow::Error>);
impl std::fmt::Display for SharedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "A background connection or validation operation failed during pool acquisition",
        )
    }
}
impl std::fmt::Debug for SharedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for SharedFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref().as_ref())
    }
}
impl PoolErrorSink {
    pub(super) fn since(&self, start: Instant) -> Option<SharedFailure> {
        self.0
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(at, _)| *at >= start)
            .map(|(_, error)| error.clone())
    }
}
impl bb8::ErrorSink<anyhow::Error> for PoolErrorSink {
    fn sink(&self, error: anyhow::Error) {
        // The stage-specific failure has already been logged. Do not print a potentially
        // sensitive server message here. This also covers checkout validation failures.
        let error = failure("pool_background", error);
        *self.0.lock().unwrap() = Some((Instant::now(), SharedFailure(Arc::new(error))));
    }
    fn boxed_clone(&self) -> Box<dyn bb8::ErrorSink<anyhow::Error>> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::prelude::*;

    #[test]
    fn sql_error_categories_do_not_guess_authentication_from_timeouts() {
        assert_eq!(classify_sql_code(18456), "authentication");
        assert_eq!(classify_sql_code(4060), "database_selection");
        assert_eq!(classify_sql_code(916), "database_selection");
        assert_eq!(classify_sql_code(40615), "server_firewall");
        assert_eq!(classify_sql_code(1205), "sql_server");
    }

    #[tokio::test(start_paused = true)]
    async fn timeouts_preserve_stage_and_elapsed_source() {
        for stage in ["dns", "tcp", "session_setup", "query"] {
            let error = operation::<()>(
                stage,
                Instant::now() + Duration::from_secs(15),
                std::future::pending(),
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains(&format!("{stage} timed out")));
            assert!(error
                .downcast_ref::<tokio::time::error::Elapsed>()
                .is_some());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn handshake_tracks_tls_and_login_without_crossing_tasks() {
        let _guard = TEST_TRACING_LOCK.lock().await;
        let subscriber = tracing_subscriber::registry().with(HandshakeLayer);
        async {
            let tls = handshake::<()>(Instant::now() + Duration::from_secs(15), async {
                tracing::info!(target: "tiberius::client::connection", "Performing a TLS handshake");
                tokio::task::yield_now().await;
                std::future::pending().await
            });
            let login = handshake::<()>(Instant::now() + Duration::from_secs(15), async {
                tracing::info!(target: "tiberius::client::connection", "Performing a TLS handshake");
                tracing::info!(target: "tiberius::client::connection", "TLS handshake successful");
                std::future::pending().await
            });
            let (tls, login) = tokio::join!(tls, login);
            assert!(tls.unwrap_err().to_string().contains("tls timed out"));
            assert!(login.unwrap_err().to_string().contains("login timed out"));
        }.with_subscriber(subscriber).await;
    }

    #[tokio::test(start_paused = true)]
    async fn background_errors_retain_source_without_attaching_old_failures() {
        use bb8::ErrorSink;
        let sink = PoolErrorSink::default();
        let start = Instant::now();
        sink.sink(tiberius::error::Error::Tls("test rejection".into()).into());
        let error = anyhow::Error::new(sink.since(start).unwrap());
        assert!(error
            .chain()
            .any(|source| source.downcast_ref::<tiberius::error::Error>().is_some()));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(sink.since(Instant::now()).is_none());
    }

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);
    impl Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn diagnostics_keep_original_error_but_never_log_its_sensitive_text() {
        let _guard = TEST_TRACING_LOCK.blocking_lock();
        let logs = LogBuffer::default();
        let writer = logs.clone();
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let source = tiberius::error::Error::Tls("secret-sentinel".into());
            let error = failure("tls", source.clone().into());
            assert_eq!(
                error.downcast_ref::<tiberius::error::Error>(),
                Some(&source)
            );
        });
        let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(text.contains("category=\"tls\""), "{text}");
        assert!(!text.contains("secret-sentinel"));
    }
}
