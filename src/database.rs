//! A small shared execution layer for SQLite development and Azure SQL deployment.
//! SQL dialect differences are explicit at call sites; values are always bound parameters.
use crate::config::DatabaseConfig;
use anyhow::{bail, Context};
use async_trait::async_trait;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    Column, Row as _, TypeInfo, ValueRef,
};
use std::{path::Path, time::Duration};
use tiberius::{AuthMethod, Client, ColumnData, EncryptionLevel};
use tokio::{
    net::{lookup_host, TcpStream},
    time::Instant,
};
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub mod diagnostics;
use diagnostics::{failure, handshake, operation, PoolErrorSink};
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const QUERY_TIMEOUT: Duration = Duration::from_secs(15);

fn azure_pool_builder<M: bb8::ManageConnection>() -> bb8::Builder<M> {
    bb8::Pool::builder()
        .max_size(4)
        // Eagerly connect, propagating the original error from build().
        .min_idle(Some(1))
        .retry_connection(false)
        .connection_timeout(Duration::from_secs(20))
        .max_lifetime(Some(Duration::from_secs(1800)))
        .test_on_check_out(true)
}
type TdsClient = Client<Compat<TcpStream>>;
pub enum Database {
    Sqlite(sqlx::SqlitePool),
    Azure(AzurePool),
}
pub struct AzurePool {
    pool: bb8::Pool<Manager>,
    failures: PoolErrorSink,
}
impl AzurePool {
    async fn get(&self) -> anyhow::Result<bb8::PooledConnection<'_, Manager>> {
        let start = Instant::now();
        self.pool
            .get()
            .await
            .map_err(|error| self.acquire_error(error, start))
    }
    async fn get_owned(&self) -> anyhow::Result<bb8::PooledConnection<'static, Manager>> {
        let start = Instant::now();
        self.pool
            .get_owned()
            .await
            .map_err(|error| self.acquire_error(error, start))
    }
    fn acquire_error(&self, error: bb8::RunError<anyhow::Error>, start: Instant) -> anyhow::Error {
        let error = pool_error(error, self.pool.state());
        match self.failures.since(start) {
            // A concurrent pool failure is evidence, not proof that it caused this wait.
            Some(source) => anyhow::Error::new(source).context(error),
            None => error,
        }
    }
}
pub enum Transaction {
    Sqlite(sqlx::Transaction<'static, sqlx::Sqlite>),
    Azure(bb8::PooledConnection<'static, Manager>),
}
#[derive(Clone)]
pub struct Manager {
    config: DatabaseConfig,
}
pub struct Connection {
    client: Option<TdsClient>,
    in_transaction: bool,
}

impl Database {
    pub async fn open(config: &DatabaseConfig, sqlite_path: &Path) -> anyhow::Result<Self> {
        match config.backend.as_str() {
            "sqlite" => {
                let options = SqliteConnectOptions::new()
                    .filename(sqlite_path)
                    .create_if_missing(true)
                    .foreign_keys(true)
                    .journal_mode(SqliteJournalMode::Wal)
                    .busy_timeout(Duration::from_secs(5));
                let pool = SqlitePoolOptions::new()
                    .max_connections(1)
                    .connect_with(options)
                    .await?;
                sqlx::migrate!("./migrations/sqlite").run(&pool).await?;
                Ok(Self::Sqlite(pool))
            }
            "azure_sql" => {
                config.validate()?;
                let failures = PoolErrorSink::default();
                let pool = azure_pool_builder()
                    .error_sink(Box::new(failures.clone()))
                    .build(Manager {
                        config: config.clone(),
                    })
                    .await
                    .context("Azure SQL initial connection failed")?;
                let database = Self::Azure(AzurePool { pool, failures });
                if config.migrate {
                    database.migrate().await?;
                } else {
                    query_scalar::<i64>("SELECT version FROM journal_schema WHERE version=6").fetch_one(&database).await.context("Azure SQL schema verification failed; inspect the underlying error before running --migrate")?;
                }
                Ok(database)
            }
            _ => bail!("database.backend must be sqlite or azure_sql"),
        }
    }
    pub fn backend(&self) -> &'static str {
        match self {
            Self::Sqlite(_) => "sqlite",
            Self::Azure(_) => "azure_sql",
        }
    }
    pub async fn begin(&self) -> anyhow::Result<Transaction> {
        match self {
            Self::Sqlite(pool) => Ok(Transaction::Sqlite(pool.begin().await?)),
            Self::Azure(pool) => {
                let mut connection = pool.get_owned().await?;
                // A dropped/cancelled transaction is never returned as a reusable connection.
                connection.in_transaction = true;
                connection
                    .transaction_command(TransactionCommand::Begin)
                    .await?;
                Ok(Transaction::Azure(connection))
            }
        }
    }
    pub async fn close(&self) {
        if let Self::Sqlite(pool) = self {
            pool.close().await;
        }
    }
    async fn migrate(&self) -> anyhow::Result<()> {
        let mut tx = self.begin().await?;
        query("DECLARE @result int; EXEC @result = sys.sp_getapplock @Resource=N'plant-journal-schema', @LockMode='Exclusive', @LockOwner='Transaction', @LockTimeout=15000; IF @result < 0 THROW 50001, 'Could not acquire schema migration lock', 1;").execute(&mut tx).await?;
        query("IF OBJECT_ID(N'dbo.journal_schema', N'U') IS NULL CREATE TABLE dbo.journal_schema (version bigint NOT NULL PRIMARY KEY, applied_at datetime2 NOT NULL DEFAULT SYSUTCDATETIME());").execute(&mut tx).await?;
        let exists: i64 = query_scalar("SELECT COUNT(*) FROM journal_schema WHERE version=1")
            .fetch_one(&mut tx)
            .await?;
        if exists == 0 {
            query(include_str!("../migrations/azure_sql/0001_initial.sql"))
                .execute(&mut tx)
                .await?;
            query("INSERT INTO journal_schema(version) VALUES(1)")
                .execute(&mut tx)
                .await?;
        }
        let exists: i64 = query_scalar("SELECT COUNT(*) FROM journal_schema WHERE version=2")
            .fetch_one(&mut tx)
            .await?;
        if exists == 0 {
            query(include_str!(
                "../migrations/azure_sql/0002_seed_inventory.sql"
            ))
            .execute(&mut tx)
            .await?;
            query("INSERT INTO journal_schema(version) VALUES(2)")
                .execute(&mut tx)
                .await?;
        }
        let exists: i64 = query_scalar("SELECT COUNT(*) FROM journal_schema WHERE version=3")
            .fetch_one(&mut tx)
            .await?;
        if exists == 0 {
            query(include_str!("../migrations/azure_sql/0003_seed_photos.sql"))
                .execute(&mut tx)
                .await?;
            query("INSERT INTO journal_schema(version) VALUES(3)")
                .execute(&mut tx)
                .await?;
        }
        let exists: i64 = query_scalar("SELECT COUNT(*) FROM journal_schema WHERE version=4")
            .fetch_one(&mut tx)
            .await?;
        if exists == 0 {
            query(include_str!("../migrations/azure_sql/0004_gardens.sql"))
                .execute(&mut tx)
                .await?;
            query("INSERT INTO journal_schema(version) VALUES(4)")
                .execute(&mut tx)
                .await?;
        }
        let exists: i64 = query_scalar("SELECT COUNT(*) FROM journal_schema WHERE version=5")
            .fetch_one(&mut tx)
            .await?;
        if exists == 0 {
            query(include_str!("../migrations/azure_sql/0005_strains.sql"))
                .execute(&mut tx)
                .await?;
            query("INSERT INTO journal_schema(version) VALUES(5)")
                .execute(&mut tx)
                .await?;
        }
        let exists: i64 = query_scalar("SELECT COUNT(*) FROM journal_schema WHERE version=6")
            .fetch_one(&mut tx)
            .await?;
        if exists == 0 {
            query(include_str!(
                "../migrations/azure_sql/0006_catalog_version.sql"
            ))
            .execute(&mut tx)
            .await?;
            query("INSERT INTO journal_schema(version) VALUES(6)")
                .execute(&mut tx)
                .await?;
        }
        tx.commit().await
    }
}
impl Transaction {
    pub async fn commit(self) -> anyhow::Result<()> {
        match self {
            Self::Sqlite(tx) => {
                tx.commit().await?;
            }
            Self::Azure(mut connection) => {
                connection
                    .transaction_command(TransactionCommand::Commit)
                    .await?;
                connection.in_transaction = false;
            }
        }
        Ok(())
    }
}
#[async_trait]
impl bb8::ManageConnection for Manager {
    type Connection = Connection;
    type Error = anyhow::Error;
    async fn connect(&self) -> anyhow::Result<Connection> {
        let config = &self.config;
        let mut tds = tiberius::Config::new();
        tds.host(config.server());
        tds.port(config.port);
        tds.database(config.name());
        tds.application_name("plant-journal");
        tds.encryption(EncryptionLevel::Required);
        if let Some(path) = &config.ca_certificate {
            tds.trust_cert_ca(path.to_string_lossy());
        }
        let username = std::env::var(&config.username_env)
            .with_context(|| format!("Set the {} environment variable", config.username_env))?;
        let password = std::env::var(&config.password_env)
            .with_context(|| format!("Set the {} environment variable", config.password_env))?;
        tds.authentication(AuthMethod::sql_server(username, password));
        connect_tds(tds, &config.server(), config.port, CONNECT_TIMEOUT).await
    }

    async fn is_valid(&self, connection: &mut Connection) -> anyhow::Result<()> {
        connection
            .run_stage(statement("SELECT 1"), true, "pool_validation")
            .await?;
        Ok(())
    }
    fn has_broken(&self, connection: &mut Connection) -> bool {
        connection.client.is_none() || connection.in_transaction
    }
}
/// Use the same driver and TLS/login path as the pool, without migrations or hardware workers.
/// Successful completion proves SQL authentication, database selection, and a round trip.
pub async fn check_connection(config: &DatabaseConfig) -> anyhow::Result<()> {
    use bb8::ManageConnection;
    anyhow::ensure!(config.backend == "azure_sql", "--check-database requires database.backend=azure_sql; export PLANT_CONFIG and load .env first (set -a; source .env; set +a)");
    config.validate()?;
    tracing::info!(server = %config.server(), database = %config.name(), port = config.port,
        authentication = "sql_password", encryption = "required", certificate_validation = true,
        "Checking Azure SQL connection (no migrations)");
    let mut connection = Manager {
        config: config.clone(),
    }
    .connect()
    .await?;
    let result = connection
        .run(statement("SELECT CAST(1 AS bigint), DB_NAME()"), true)
        .await?;
    let row = result
        .rows
        .first()
        .context("Connectivity query returned no row")?;
    anyhow::ensure!(
        i64::decode(&row.values[0])? == 1,
        "Connectivity query did not return 1"
    );
    anyhow::ensure!(
        String::decode(&row.values[1])? == config.name(),
        "Connected to an unexpected database"
    );
    tracing::info!(
        stage = "select_1",
        "Azure SQL connectivity verified, including database selection"
    );
    Ok(())
}

async fn connect_tds(
    mut tds: tiberius::Config,
    initial_host: &str,
    initial_port: u16,
    timeout: Duration,
) -> anyhow::Result<Connection> {
    // A single budget includes all redirects, TLS/login, and session setup.
    let deadline = Instant::now() + timeout;
    let mut host = initial_host.to_owned();
    let mut port = initial_port;
    for redirects in 0..=2 {
        tracing::info!(server = %host, port, redirects, "Connecting to Azure SQL endpoint");
        let addresses = operation("dns", deadline, async {
            let addresses: Vec<_> = lookup_host((host.as_str(), port)).await?.collect();
            anyhow::ensure!(!addresses.is_empty(), "DNS returned no addresses");
            Ok(addresses)
        })
        .await?;
        let tcp = operation("tcp", deadline, async {
            let tcp = TcpStream::connect(addresses.as_slice()).await?;
            tcp.set_nodelay(true)?;
            Ok(tcp)
        })
        .await?;
        tracing::info!(
            stage = "tds_prelogin",
            "Starting TDS prelogin, TLS, and SQL login"
        );
        match handshake(deadline, Client::connect(tds.clone(), tcp.compat_write())).await {
            Ok(mut client) => {
                tracing::info!(
                    stage = "login",
                    "SQL login and database selection completed"
                );
                operation("session_setup", deadline, async {
                    client
                        .simple_query("SET XACT_ABORT ON; SET NOCOUNT OFF;")
                        .await?
                        .into_results()
                        .await?;
                    Ok(())
                })
                .await?;
                return Ok(Connection {
                    client: Some(client),
                    in_transaction: false,
                });
            }
            Err(error) => match error.downcast_ref::<tiberius::error::Error>() {
                Some(tiberius::error::Error::Routing {
                    host: next_host,
                    port: next_port,
                }) if redirects < 2 => {
                    // This is TDS routing, not a retry. Preserve credentials, required TLS,
                    // certificate validation, selected database, and the original deadline.
                    host.clone_from(next_host);
                    port = *next_port;
                    tds.host(&host);
                    tds.port(port);
                    tracing::info!(server = %host, port, stage = "routing", "Following Azure SQL login redirect");
                }
                Some(tiberius::error::Error::Routing { .. }) => {
                    return Err(failure(
                        "routing",
                        error.context("Azure SQL redirect limit exceeded"),
                    ))
                }
                _ => return Err(error),
            },
        }
    }
    unreachable!("the final redirect either connects or returns an error")
}

enum TransactionCommand {
    Begin,
    Commit,
}

impl Connection {
    async fn transaction_command(&mut self, command: TransactionCommand) -> anyhow::Result<()> {
        let (sql, stage) = match command {
            TransactionCommand::Begin => ("BEGIN TRANSACTION", "transaction_begin"),
            TransactionCommand::Commit => ("COMMIT TRANSACTION", "transaction_commit"),
        };
        let mut client = self
            .client
            .take()
            .context("Database connection is no longer usable")?;
        // Query::execute wraps SQL in sp_executesql. Transaction boundaries must be
        // top-level batches: changing @@TRANCOUNT across an RPC causes SQL error 266.
        // Only these constant statements use this path; application values stay bound.
        let result = operation(stage, Instant::now() + QUERY_TIMEOUT, async {
            client.simple_query(sql).await?.into_results().await?;
            Ok(())
        })
        .await;
        // As with ordinary queries, failed/cancelled commands discard the socket.
        // In particular, never retry COMMIT when its outcome could be uncertain.
        if result.is_ok() {
            self.client = Some(client);
        }
        result
    }

    async fn run(&mut self, statement: Statement, fetch: bool) -> anyhow::Result<Output> {
        self.run_stage(statement, fetch, "query").await
    }
    async fn run_stage(
        &mut self,
        statement: Statement,
        fetch: bool,
        stage: &'static str,
    ) -> anyhow::Result<Output> {
        let mut client = self
            .client
            .take()
            .context("Database connection is no longer usable")?;
        // On cancellation, timeout, or any protocol/query error, drop this socket. Never replay a write.
        let result = operation(stage, Instant::now() + QUERY_TIMEOUT, async {
            let sql = parameters(statement.azure_sql.as_deref().unwrap_or(&statement.sql));
            let mut query = tiberius::Query::new(sql);
            for value in statement.values {
                match value {
                    Value::Null => query.bind(None::<String>),
                    Value::Text(v) => query.bind(v),
                    Value::Int(v) => query.bind(v),
                    Value::Real(v) => query.bind(v),
                    Value::Bool(v) => query.bind(v),
                }
            }
            if fetch {
                let rows = query.query(&mut client).await?.into_first_result().await?;
                let rows = rows
                    .into_iter()
                    .map(tds_row)
                    .collect::<anyhow::Result<Vec<_>>>()?;
                Ok(Output { rows, affected: 0 })
            } else {
                Ok(Output {
                    rows: vec![],
                    affected: query.execute(&mut client).await?.total(),
                })
            }
        })
        .await;
        if result.is_ok() {
            self.client = Some(client);
        }
        result
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Text(String),
    Int(i64),
    Real(f64),
    Bool(bool),
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}
impl From<&String> for Value {
    fn from(v: &String) -> Self {
        Self::Text(v.clone())
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::Text(v.into())
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Self {
        Self::Int(v.into())
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Self::Real(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map(Into::into).unwrap_or(Self::Null)
    }
}
impl<T: Clone + Into<Value>> From<&Option<T>> for Value {
    fn from(v: &Option<T>) -> Self {
        v.clone().into()
    }
}
#[derive(Debug)]
pub struct Record {
    pub columns: Vec<String>,
    pub values: Vec<Value>,
}
impl Record {
    pub fn get<T: Decode>(&self, name: &str) -> anyhow::Result<T> {
        let index = self
            .columns
            .iter()
            .position(|c| c == name)
            .with_context(|| format!("Missing column {name}"))?;
        T::decode(&self.values[index])
    }
    pub fn first<T: Decode>(&self) -> anyhow::Result<T> {
        T::decode(self.values.first().context("Query returned no columns")?)
    }
}
pub trait Decode: Sized {
    fn decode(value: &Value) -> anyhow::Result<Self>;
}
impl Decode for String {
    fn decode(v: &Value) -> anyhow::Result<Self> {
        match v {
            Value::Text(v) => Ok(v.clone()),
            _ => bail!("Expected a string database value"),
        }
    }
}
impl Decode for i64 {
    fn decode(v: &Value) -> anyhow::Result<Self> {
        match v {
            Value::Int(v) => Ok(*v),
            _ => bail!("Expected an integer database value"),
        }
    }
}
impl Decode for f64 {
    fn decode(v: &Value) -> anyhow::Result<Self> {
        match v {
            Value::Real(v) => Ok(*v),
            Value::Int(v) => Ok(*v as f64),
            _ => bail!("Expected a numeric database value"),
        }
    }
}
impl Decode for bool {
    fn decode(v: &Value) -> anyhow::Result<Self> {
        match v {
            Value::Bool(v) => Ok(*v),
            Value::Int(0) => Ok(false),
            Value::Int(1) => Ok(true),
            _ => bail!("Expected a boolean database value"),
        }
    }
}
impl<T: Decode> Decode for Option<T> {
    fn decode(v: &Value) -> anyhow::Result<Self> {
        if *v == Value::Null {
            Ok(None)
        } else {
            Ok(Some(T::decode(v)?))
        }
    }
}
pub trait FromRecord: Sized {
    fn from_record(row: &Record) -> anyhow::Result<Self>;
}
impl FromRecord for Record {
    fn from_record(row: &Record) -> anyhow::Result<Self> {
        Ok(Self {
            columns: row.columns.clone(),
            values: row.values.clone(),
        })
    }
}

pub struct Statement {
    sql: String,
    azure_sql: Option<String>,
    values: Vec<Value>,
}
fn statement(sql: &str) -> Statement {
    Statement {
        sql: sql.into(),
        azure_sql: None,
        values: vec![],
    }
}
pub struct Query<T> {
    statement: Statement,
    decode: fn(&Record) -> anyhow::Result<T>,
}
pub struct Output {
    rows: Vec<Record>,
    affected: u64,
}
impl Output {
    pub fn rows_affected(&self) -> u64 {
        self.affected
    }
}
pub fn query(sql: &str) -> Query<()> {
    Query {
        statement: statement(sql),
        decode: |_| Ok(()),
    }
}
pub fn query_as<T: FromRecord>(sql: &str) -> Query<T> {
    Query {
        statement: statement(sql),
        decode: T::from_record,
    }
}
pub fn query_scalar<T: Decode>(sql: &str) -> Query<T> {
    Query {
        statement: statement(sql),
        decode: Record::first,
    }
}
impl<T> Query<T> {
    pub fn sql_server(mut self, sql: &str) -> Self {
        self.statement.azure_sql = Some(sql.into());
        self
    }
    pub fn bind(mut self, value: impl Into<Value>) -> Self {
        self.statement.values.push(value.into());
        self
    }
    pub async fn execute(self, executor: impl Executor) -> anyhow::Result<Output> {
        executor.run(self.statement, false).await
    }
    pub async fn fetch_all(self, executor: impl Executor) -> anyhow::Result<Vec<T>> {
        executor
            .run(self.statement, true)
            .await?
            .rows
            .iter()
            .map(self.decode)
            .collect()
    }
    pub async fn fetch_optional(self, executor: impl Executor) -> anyhow::Result<Option<T>> {
        executor
            .run(self.statement, true)
            .await?
            .rows
            .first()
            .map(self.decode)
            .transpose()
    }
    pub async fn fetch_one(self, executor: impl Executor) -> anyhow::Result<T> {
        self.fetch_optional(executor)
            .await?
            .context("Database record not found")
    }
}
#[async_trait]
pub trait Executor: Send {
    async fn run(self, statement: Statement, fetch: bool) -> anyhow::Result<Output>;
}
#[async_trait]
impl Executor for &Database {
    async fn run(self, statement: Statement, fetch: bool) -> anyhow::Result<Output> {
        match self {
            Database::Sqlite(pool) => {
                sqlite_run(&mut *pool.acquire().await?, statement, fetch).await
            }
            Database::Azure(pool) => pool.get().await?.run(statement, fetch).await,
        }
    }
}
#[async_trait]
impl Executor for &mut Transaction {
    async fn run(self, statement: Statement, fetch: bool) -> anyhow::Result<Output> {
        match self {
            Transaction::Sqlite(tx) => sqlite_run(&mut *tx, statement, fetch).await,
            Transaction::Azure(connection) => connection.run(statement, fetch).await,
        }
    }
}
async fn sqlite_run(
    connection: &mut sqlx::SqliteConnection,
    statement: Statement,
    fetch: bool,
) -> anyhow::Result<Output> {
    let mut query = sqlx::query(&statement.sql);
    for v in statement.values {
        query = match v {
            Value::Null => query.bind(None::<String>),
            Value::Text(v) => query.bind(v),
            Value::Int(v) => query.bind(v),
            Value::Real(v) => query.bind(v),
            Value::Bool(v) => query.bind(v),
        };
    }
    if fetch {
        Ok(Output {
            rows: query
                .fetch_all(connection)
                .await?
                .into_iter()
                .map(sqlite_row)
                .collect::<anyhow::Result<Vec<_>>>()?,
            affected: 0,
        })
    } else {
        Ok(Output {
            rows: vec![],
            affected: query.execute(connection).await?.rows_affected(),
        })
    }
}
pub(crate) fn sqlite_row(row: sqlx::sqlite::SqliteRow) -> anyhow::Result<Record> {
    let columns = row.columns().iter().map(|c| c.name().to_owned()).collect();
    let mut values = vec![];
    for i in 0..row.len() {
        let raw = row.try_get_raw(i)?;
        let value = if raw.is_null() {
            Value::Null
        } else {
            match raw.type_info().name() {
                "TEXT" => Value::Text(row.try_get(i)?),
                "INTEGER" => Value::Int(row.try_get(i)?),
                "REAL" => Value::Real(row.try_get(i)?),
                "BOOLEAN" => Value::Bool(row.try_get(i)?),
                other => bail!("Unsupported SQLite column type {other}"),
            }
        };
        values.push(value);
    }
    Ok(Record { columns, values })
}
fn tds_row(row: tiberius::Row) -> anyhow::Result<Record> {
    let columns = row.columns().iter().map(|c| c.name().to_owned()).collect();
    let mut values = vec![];
    for v in row {
        values.push(match v {
            ColumnData::U8(v) => v.map(|v| Value::Int(v.into())).unwrap_or(Value::Null),
            ColumnData::I16(v) => v.map(|v| Value::Int(v.into())).unwrap_or(Value::Null),
            ColumnData::I32(v) => v.map(|v| Value::Int(v.into())).unwrap_or(Value::Null),
            ColumnData::I64(v) => v.map(Value::Int).unwrap_or(Value::Null),
            ColumnData::F32(v) => v.map(|v| Value::Real(v.into())).unwrap_or(Value::Null),
            ColumnData::F64(v) => v.map(Value::Real).unwrap_or(Value::Null),
            ColumnData::Bit(v) => v.map(Value::Bool).unwrap_or(Value::Null),
            ColumnData::String(v) => v
                .map(|v| Value::Text(v.into_owned()))
                .unwrap_or(Value::Null),
            _ => bail!("Unsupported SQL Server column type"),
        });
    }
    Ok(Record { columns, values })
}
/// Replace positional placeholders, preserving quoted literals/identifiers and comments.
pub fn parameters(sql: &str) -> String {
    let mut result = String::new();
    let mut chars = sql.chars().peekable();
    let mut index = 0;
    let mut quote = None;
    let mut line_comment = false;
    let mut block_depth = 0;
    while let Some(c) = chars.next() {
        if line_comment {
            result.push(c);
            if c == '\n' {
                line_comment = false;
            }
            continue;
        }
        if block_depth > 0 {
            result.push(c);
            if c == '*' && chars.peek() == Some(&'/') {
                result.push(chars.next().unwrap());
                block_depth -= 1;
            } else if c == '/' && chars.peek() == Some(&'*') {
                result.push(chars.next().unwrap());
                block_depth += 1;
            }
            continue;
        }
        if let Some(end) = quote {
            result.push(c);
            if c == end {
                if chars.peek() == Some(&end) {
                    result.push(chars.next().unwrap());
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                result.push(c);
            }
            '[' => {
                quote = Some(']');
                result.push(c);
            }
            '-' if chars.peek() == Some(&'-') => {
                result.push(c);
                result.push(chars.next().unwrap());
                line_comment = true;
            }
            '/' if chars.peek() == Some(&'*') => {
                result.push(c);
                result.push(chars.next().unwrap());
                block_depth = 1;
            }
            '?' => {
                index += 1;
                result.push_str(&format!("@P{index}"));
            }
            _ => result.push(c),
        }
    }
    result
}

impl FromRecord for (i64, i64, i64) {
    fn from_record(row: &Record) -> anyhow::Result<Self> {
        anyhow::ensure!(row.values.len() == 3, "Expected three database columns");
        Ok((
            i64::decode(&row.values[0])?,
            i64::decode(&row.values[1])?,
            i64::decode(&row.values[2])?,
        ))
    }
}

fn pool_error(error: bb8::RunError<anyhow::Error>, state: bb8::State) -> anyhow::Error {
    tracing::error!(
        stage = "pool_acquire",
        connections = state.connections,
        idle_connections = state.idle_connections,
        "Azure SQL pool acquisition failed"
    );
    match error {
        bb8::RunError::User(error) => error.context("Azure SQL pool acquisition failed"),
        bb8::RunError::TimedOut => anyhow::anyhow!(
            "Azure SQL pool acquisition timed out after 20s (connections={}, idle={}); inspect preceding connection/validation stage errors; if none, check long-held transactions or exhausted pool",
            state.connections, state.idle_connections
        ),
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::prelude::*;

    #[derive(Clone)]
    struct FakeManager {
        fail: bool,
        attempts: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl bb8::ManageConnection for FakeManager {
        type Connection = ();
        type Error = anyhow::Error;
        async fn connect(&self) -> anyhow::Result<()> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(tiberius::error::Error::Tls("test TLS rejection".into()).into());
            }
            Ok(())
        }
        async fn is_valid(&self, _: &mut ()) -> anyhow::Result<()> {
            Ok(())
        }
        fn has_broken(&self, _: &mut ()) -> bool {
            false
        }
    }

    #[tokio::test(start_paused = true)]
    async fn lazy_pool_masks_errors_but_eager_pool_returns_original_without_retry() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let manager = FakeManager {
            fail: true,
            attempts: attempts.clone(),
        };
        // Reproduce the original setup: lazy build succeeds; get loses the TLS error.
        let old = bb8::Pool::builder()
            .connection_timeout(Duration::from_secs(20))
            .build(manager.clone())
            .await
            .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert!(matches!(old.get().await, Err(bb8::RunError::TimedOut)));
        let attempts = Arc::new(AtomicUsize::new(0));
        let error = azure_pool_builder()
            .build(FakeManager {
                fail: true,
                attempts: attempts.clone(),
            })
            .await
            .err()
            .unwrap();
        assert!(matches!(
            error.downcast_ref(),
            Some(tiberius::error::Error::Tls(_))
        ));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn exhausted_pool_reports_acquisition_and_occupancy() {
        let pool = azure_pool_builder()
            .max_size(1)
            .build(FakeManager {
                fail: false,
                attempts: Arc::new(AtomicUsize::new(0)),
            })
            .await
            .unwrap();
        let _held = pool.get().await.unwrap();
        let error = pool_error(pool.get().await.err().unwrap(), pool.state());
        let message = error.to_string();
        assert!(message.contains("pool acquisition timed out"));
        assert!(message.contains("connections=1, idle=0"));
        assert!(!message.contains("authentication"));
    }

    fn fake_config(port: u16) -> tiberius::Config {
        let mut config = tiberius::Config::new();
        config.host("localhost");
        config.port(port);
        config.database("ripsql");
        config.encryption(EncryptionLevel::Required);
        config.authentication(AuthMethod::sql_server("fake-user", "fake-password"));
        config
    }

    #[tokio::test]
    async fn invalid_dns_name_preserves_resolution_error() {
        let error = connect_tds(fake_config(1433), "invalid\0host", 1433, CONNECT_TIMEOUT)
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("dns failed"));
        assert!(error.downcast_ref::<std::io::Error>().is_some());
    }

    #[tokio::test]
    async fn refused_socket_reports_tcp_and_preserves_io_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let error = connect_tds(fake_config(port), "127.0.0.1", port, CONNECT_TIMEOUT)
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("tcp failed"));
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::ConnectionRefused
        );
    }

    #[tokio::test]
    async fn tcp_success_does_not_prove_tds_or_tls_success() {
        let _guard = diagnostics::TEST_TRACING_LOCK.lock().await;
        for respond_to_prelogin in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut header = [0; 8];
                socket.read_exact(&mut header).await.unwrap();
                assert_eq!(header[0], 0x12); // TDS PRELOGIN, not a raw TLS ClientHello
                let size = u16::from_be_bytes([header[2], header[3]]) as usize;
                let mut payload = vec![0; size - 8];
                socket.read_exact(&mut payload).await.unwrap();
                if respond_to_prelogin {
                    // TDS response with ENCRYPTION=REQUIRED; then stall during real TLS negotiation.
                    socket
                        .write_all(&[4, 1, 0, 15, 0, 0, 1, 0, 1, 0, 6, 0, 1, 0xff, 3])
                        .await
                        .unwrap();
                }
                std::future::pending::<()>().await;
            });
            let error = connect_tds(fake_config(port), "127.0.0.1", port, Duration::from_secs(2))
                .with_subscriber(tracing_subscriber::registry().with(diagnostics::HandshakeLayer))
                .await
                .err()
                .unwrap();
            server.abort();
            assert!(
                error
                    .downcast_ref::<tokio::time::error::Elapsed>()
                    .is_some(),
                "{error:#}"
            );
            let stage = if respond_to_prelogin {
                "tls"
            } else {
                "tds_prelogin_or_handshake"
            };
            assert!(
                error.to_string().contains(&format!("{stage} timed out")),
                "{error:#}"
            );
        }
    }
}

#[cfg(test)]
mod transaction_wire_tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    async fn packet(socket: &mut TcpStream) -> (u8, Vec<u8>) {
        let mut header = [0; 8];
        socket.read_exact(&mut header).await.unwrap();
        assert_eq!(header[1] & 1, 1, "fixture expects one complete packet");
        let length = u16::from_be_bytes([header[2], header[3]]) as usize;
        let mut payload = vec![0; length - 8];
        socket.read_exact(&mut payload).await.unwrap();
        (header[0], payload)
    }
    async fn reply(socket: &mut TcpStream, payload: &[u8]) {
        let length = (payload.len() + 8) as u16;
        let [high, low] = length.to_be_bytes();
        socket
            .write_all(&[4, 1, high, low, 0, 0, 1, 0])
            .await
            .unwrap();
        socket.write_all(payload).await.unwrap();
    }
    const DONE: [u8; 13] = [0xfd, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

    async fn exercise_transaction(fail_commit: bool) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            assert_eq!(packet(&mut socket).await.0, 0x12); // PRELOGIN
            reply(&mut socket, &[1, 0, 6, 0, 1, 0xff, 2]).await;
            assert_eq!(packet(&mut socket).await.0, 0x10); // LOGIN7
            reply(&mut socket, &DONE).await;

            let (kind, payload) = packet(&mut socket).await;
            assert_eq!(kind, 1, "BEGIN must be SQLBatch, not sp_executesql RPC");
            let text = String::from_utf16(
                &payload[22..]
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(text, "BEGIN TRANSACTION");
            // BeginTransaction ENVCHANGE supplies the descriptor used on subsequent requests.
            let mut response = vec![0xe3, 11, 0, 8, 8];
            response.extend_from_slice(&[7; 8]);
            response.push(0);
            response.extend_from_slice(&DONE);
            reply(&mut socket, &response).await;

            let (kind, payload) = packet(&mut socket).await;
            assert_eq!(kind, 3, "data query must still use parameterized RPC");
            assert_eq!(&payload[10..18], &[7; 8]);
            reply(&mut socket, &DONE).await;

            let (kind, payload) = packet(&mut socket).await;
            assert_eq!(kind, 1, "COMMIT must be SQLBatch, not sp_executesql RPC");
            assert_eq!(&payload[10..18], &[7; 8]);
            let text = String::from_utf16(
                &payload[22..]
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(text, "COMMIT TRANSACTION");
            if !fail_commit {
                let mut response = vec![0xe3, 3, 0, 9, 0, 0];
                response.extend_from_slice(&DONE);
                reply(&mut socket, &response).await;
            }
            // In the failure case, close without acknowledging COMMIT.
        });
        // Protocol-only loopback fixture with fake credentials. Production continues
        // to require verified TLS; no configuration/env or external server is used.
        let mut config = tiberius::Config::new();
        config.encryption(EncryptionLevel::NotSupported);
        config.authentication(AuthMethod::sql_server("fixture", "fixture"));
        let tcp = TcpStream::connect(address).await.unwrap();
        let client = Client::connect(config, tcp.compat_write()).await.unwrap();
        let mut connection = Connection {
            client: Some(client),
            in_transaction: true,
        };
        connection
            .transaction_command(TransactionCommand::Begin)
            .await
            .unwrap();
        let mut update = statement("UPDATE plants SET name=?");
        update.values.push(Value::Text("O'Brien".into()));
        connection.run(update, false).await.unwrap();
        let result = connection
            .transaction_command(TransactionCommand::Commit)
            .await;
        if fail_commit {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("transaction_commit"));
            assert!(
                connection.client.is_none(),
                "uncertain COMMIT must discard the socket"
            );
        } else {
            result.unwrap();
            assert!(connection.client.is_some());
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn transaction_boundaries_use_batches_and_preserve_descriptor() {
        tokio::time::timeout(Duration::from_secs(5), exercise_transaction(false))
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn failed_commit_discards_connection_without_retry() {
        tokio::time::timeout(Duration::from_secs(5), exercise_transaction(true))
            .await
            .unwrap();
    }
}
