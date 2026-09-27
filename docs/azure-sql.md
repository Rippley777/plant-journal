# Hosted Azure SQL database

The Rust service can use Azure SQL Database (the Microsoft SQL Server engine) for journal data, plants, photo metadata, sensor samples, schedules, overrides, and execution history. The web server and hardware adapters still run on the Pi. **Photo files stay on the Pi** in `data_dir/photos`; Azure Blob Storage is not part of this change.

The driver is Tiberius with a bounded bb8 connection pool. It speaks TDS over TCP with required TLS and certificate/hostname validation. Native ODBC drivers are not required. The current implementation uses SQL username/password authentication from environment variables; Microsoft Entra authentication is not implemented.

SQLite remains available for offline development and existing installations. Selecting Azure SQL never silently falls back to SQLite.

## Connect an Azure database

1. Create a logical SQL server and a dedicated Azure SQL database, or use an existing empty database. The application does not create Azure resources or choose a billing tier.
2. Configure a SQL identity with access to that database. Initial schema creation requires DDL permission (for example, membership in `db_ddladmin`) plus data read/write permission. After migrations, use a runtime identity in `db_datareader` and `db_datawriter`, with `database.migrate = false`.
3. Permit the Pi's outbound public IP in the SQL server firewall, or configure routing/DNS to a private endpoint. Local Wi-Fi reachability alone does not provide Azure connectivity. Azure's connection policy may also require redirect ports; the driver follows login redirects. See [Microsoft's connectivity architecture](https://learn.microsoft.com/en-us/azure/azure-sql/database/connectivity-architecture?view=azuresql).
4. Copy `config.azure.example.toml` to `config.toml` and supply the environment variables below. Credentials belong in the environment, not TOML or Git.

```sh
export AZURE_SQL_SERVER='your-server.database.windows.net'
export AZURE_SQL_DATABASE='plant-journal'
export AZURE_SQL_USERNAME='your-database-user'
# Set AZURE_SQL_PASSWORD through your shell's secret input or secret manager.
export PLANT_CONFIG=config.toml
cargo run --locked -- --migrate
cargo run --locked
```

`AZURE_SQL_PASSWORD` must be set as well. Do not put real credentials in example commands committed to this repository. `server` and `name` in TOML can replace the corresponding environment variables; environment variables take precedence. Credentials always come from the names configured by `username_env` and `password_env`.

The `--migrate` command exits after schema setup, without starting the HTTP server or hardware workers. Migrations execute in a transaction under a database application lock and are versioned in `dbo.journal_schema`. Normal startup also migrates when `database.migrate = true`; with `false`, it verifies schema version 3 is installed. Use a dedicated database: table names are under `dbo` and could collide with unrelated application tables.

The default Rust startup (`cargo run` without `PLANT_CONFIG`) still uses SQLite, so existing journals and developer tests work without Azure credentials. The supplied Pi deployment configuration selects Azure SQL explicitly.

## Diagnose a connection without migrations

From the repository directory, load your local environment into the **same shell that launches the application**:

```sh
set -a
source .env
set +a
cargo run --locked -- --check-database
```

The program does not automatically read `.env`. `source .env` without export (or `set -a`) does not reliably pass new variables to Rust. An IDE launch needs its own exported environment; systemd uses its `EnvironmentFile`. No Docker networking is configured by this repository. Missing credential variables fail validation before pool creation, so that alone cannot explain the original pool timeout. Without `PLANT_CONFIG`, normal startup selects the default SQLite configuration.

The diagnostic prints the resolved server, port, database, and authentication mode, never credentials. It connects through the actual Tiberius TDS/TLS path, runs session settings and `SELECT CAST(1 AS bigint), DB_NAME()`, verifies the database, then exits. It does not migrate, create application tables, start equipment workers, or create data/photo directories.

For a contained SQL user such as `sqladmin` in `ripsql`, set `AZURE_SQL_DATABASE=ripsql` (or `database.name = "ripsql"` without an environment override). The application explicitly sets the database in the login packet. Do not change it to `master`. `AZURE_SQL_USERNAME` / `AZURE_SQL_PASSWORD` select SQL password authentication, not Microsoft Entra authentication; Entra-only servers will reject it. An authentication rejection does not necessarily mean a wrong password: database access can also cause login failure. See [Microsoft's 18456 explanation](https://learn.microsoft.com/en-us/sql/relational-databases/errors-events/mssqlserver-18456-database-engine-error).

### Why the previous timeout was misleading

The old pool configuration had no minimum connections, enabled bb8's default connection retries, and used its default no-op error sink. `build()` therefore succeeded without connecting. The first migration or schema query called `get()`, which started connections in the background. Driver errors were discarded; after 20 seconds, `get()` returned `RunError::TimedOut`, producing the generic firewall/authentication message. That message identified **pool acquisition**, not the underlying failure.

Startup now eagerly establishes one connection, with connection retries disabled, so `build()` returns the original contextual error immediately when a connection fails. Background failures after startup are logged, and a failure observed during an unsuccessful acquisition is retained in its error chain. Pool timeouts include connection/idle counts; an unrelated earlier failure is not attached. No timeout was increased. The regression tests reproduce the old behavior using a failing fake driver.

### SQL error 266 when starting a transaction

`Previous count = 0, current count = 1` during startup was caused by sending `BEGIN TRANSACTION` through Tiberius's `Query::execute`, which issues an `sp_executesql` RPC. The procedure returned with a changed transaction count. Transaction boundaries now use `Client::simple_query` to send constant `BEGIN TRANSACTION` / `COMMIT TRANSACTION` SQL batches on the same pooled connection. Data queries continue to bind parameters. Responses are fully consumed so Tiberius updates the transaction descriptor before the next request. Failed or cancelled boundaries discard the connection; commits are never retried automatically.

This error means a connection reached SQL execution; it is distinct from a login timeout. Boundary failures now report `transaction_begin` or `transaction_commit`. After updating the binary, rerun `cargo run --locked -- --migrate` with the existing exported configuration. No credential or firewall change is needed for this code defect. Local protocol tests check the actual batch/RPC packet types and transaction descriptor; the opt-in Azure test checks `@@TRANCOUNT` inside a transaction.

### Lifecycle, errors, and time budgets

[`Config::load`](../src/config.rs) reads the TOML selected by `PLANT_CONFIG`. Environment server/database overrides take precedence; credential variable names come from TOML. Missing values fail before networking. No credential-bearing connection string is constructed. [`Manager::connect`](../src/database.rs) builds a typed Tiberius configuration, requiring encryption and certificate/hostname verification. Tokio uses its multithread runtime with I/O and timers enabled.

The connection stages are DNS resolution → TCP connect → TDS prelogin → TLS → SQL login/database selection → any Azure routing redirect → session setup. Redirects repeat DNS/TCP/TDS/TLS/login for the announced endpoint with the original deadline, database, credentials, and TLS validation; at most two redirects are followed. Checkout validates the connection with `SELECT 1`. Successful queries drain results and return the connection to the pool. Query failures/cancellation and abandoned transactions discard their sockets; writes are never replayed.

| Limit | Value / scope |
| --- | --- |
| Connection establishment | 15 seconds total, including DNS, TCP, TDS, TLS, login, all redirects, and session setup |
| Pool acquisition | 20 seconds, including checkout validation; maximum 4 connections |
| Query / checkout validation | 15 seconds per operation, including response consumption |
| Schema application lock | 15 seconds server-side, also bounded by the query deadline |
| Pool lifetime / idle eviction | 30 minutes lifetime; 10 minutes idle above the minimum of 1, checked by bb8's 30-second reaper |

These are distinct limits. A saturated pool can time out without any new network attempt. A silent login timeout does not prove credentials were rejected. Error logs include `stage`, `category`, `timed_out`, SQL error code/state where available, and I/O error kind. Original driver errors remain in the returned error chain. Structured logs omit SQL text, bound values, passwords, and arbitrary server error text (which can echo data).

| Last stage / category | What it establishes and what to check next |
| --- | --- |
| `dns` | Hostname resolution failed or stalled; check the resolved configuration, DNS and VPN environment. |
| `tcp` | DNS finished but a socket could not connect; check the endpoint/port and the application's network rule. |
| `tds_prelogin_or_handshake` | TCP succeeded; no TLS milestone was observed before failure. With the standard binary's tracing enabled, inspect TDS prelogin/network handling. |
| `tls` | TDS advanced to TLS; inspect certificate/trust errors or a TLS stall. Keep validation enabled. |
| `login` | TLS completed; waiting for or processing SQL login/database selection. |
| `authentication` | SQL Server returned 18456/18452; inspect identity, authentication mode and database access. |
| `database_selection` | SQL Server returned 4060/916; inspect the explicit target database and contained-user access. |
| `server_firewall` | SQL Server explicitly returned 40615. This is evidence of a server firewall rejection. |
| `session_setup` / `query` | Login completed; a session statement or application query failed/timed out. |
| `pool_validation` / `pool_acquire` | Check checkout failures, connection counts and long-held transactions. |

Tiberius combines prelogin, TLS and login in one public `Client::connect` call. The binary's diagnostic tracing layer observes two constant TLS milestone messages from Tiberius 0.12.3 to distinguish a TLS stall from a login stall, without inspecting packets. Only the audited connection module's INFO events are enabled for this; do not enable broad driver TRACE logging to diagnose credentials. If the library is embedded without this layer, the combined `tds_prelogin_or_handshake` label is used rather than guessing the phase. Retest the milestone integration when upgrading Tiberius.

A successful `nc` only checks TCP for that process. In Little Snitch, inspect the rule/event for the executable actually launched (`target/debug/plant-journal`, a release binary, or the installed service), not just Terminal or `nc`. If logs show a routing redirect, also inspect its announced endpoint and port. For public Azure Redirect connections, outbound ports 11000–11999 may be required in addition to 1433; outside-Azure clients normally use Proxy under the Default policy. Do not change server policy without evidence. See [Microsoft's connectivity architecture](https://learn.microsoft.com/en-us/azure/azure-sql/database/connectivity-architecture?view=azuresql).

The local tests establish the error-masking defect and phase reporting, not the cause of a particular remote outage. Run this diagnostic from the failing launch environment to establish that cause; TCP success alone cannot validate TLS, authentication or database access.

## Move an existing SQLite journal

Stop the existing service and back up the entire local data directory first. Keep the old database; this is a copy, not an in-place conversion. Keep all other instances targeting either database stopped during the import.

Set the Azure environment variables and `PLANT_CONFIG` as above. Confirm `data_dir` still points to the directory containing your existing `photos/` folder, then run:

```sh
cargo run --locked -- --import-sqlite /absolute/path/to/data/journal.sqlite3
```

The command:

- Opens SQLite read-only and reads a consistent snapshot.
- Refuses to import if any destination data table is nonempty (except the initial settings row).
- Copies plants, entries, photos/associations, readings, devices, events, settings, health, and daily capture claims in one destination transaction. Sensor reading surrogate IDs are regenerated; no other table references them.
- Preserves UTC timestamps and timezone settings.
- **Disables all equipment schedules and daily photo capture, clears manual overrides, and resets reported/commanded outlet state.** Existing historical events are retained. Re-enable automation after checking the migrated setup.
- Does not upload, move, overwrite, or delete photo files or modify the source SQLite database.

On failure, the destination transaction is rolled back. If the network fails while committing, the outcome can be uncertain: inspect destination row counts before retrying. A retry against nonempty tables is refused rather than duplicating records. Never clear the original journal just because a migration command returned an error.

After importing, start the Azure-configured service, check journal/calendar entries, open existing full-size photos, verify outlet connections, and re-enable schedules deliberately. If moving the application to a different machine, copy `photos/` to the new `data_dir` as well. Run only **one controller instance per database and grow space**.

## Raspberry Pi systemd configuration

The supplied service reads `/etc/plant-journal/azure.env` when present. Copy `deploy/azure.env.example` there, fill in the real values, and restrict permissions:

```sh
sudo install -m 600 -o root -g root deploy/azure.env.example /etc/plant-journal/azure.env
sudoedit /etc/plant-journal/azure.env
```

Use systemd `EnvironmentFile` quoting for credentials containing special characters. `/etc/plant-journal/config.toml` should use `[database] backend = "azure_sql"` and the same local photo directory as before. Install the updated binary and unit, reload systemd, and restart the service after configuration:

```sh
sudo systemctl daemon-reload
sudo systemctl restart plant-journal
sudo journalctl -u plant-journal -f
```

Do not install the example environment file over a working one during later upgrades. If schema migrations are handled separately, set `migrate = false` for the runtime service after running `--migrate` with the migration identity.

## Availability and recovery

Azure mode requires a working connection to the hosted database. It is **not an offline synchronization system**:

- Startup fails if the database cannot be reached or authenticated. During an outage, API requests fail and background operations log errors. Failed samples and missed photos are not buffered for later upload.
- Equipment control reads settings and persists intent through the database. If those operations fail, new switching actions are suspended; existing outlets may stay in their last state. Even override expiry cannot be guaranteed while the database is unreachable. Use the outlet's own power-on behavior and appropriate independent equipment protections.
- When connectivity returns, later operations acquire healthy connections and reconcile the current schedule; missed transitions are not replayed.
- Connections and queries have time limits. Failed or cancelled queries and abandoned transactions discard their connection. Writes are never transparently retried because their commit outcome may be uncertain.
- If an image is finalized but its database transaction cannot be confirmed, the application retains that image. Reconcile it against the `photos` table before removing possible orphan files; a lost commit acknowledgement does not prove the record was rolled back.

For certificate issues, fix the trust chain. There is no `TrustServerCertificate=true` setting. `database.ca_certificate` can point to a CA file for a separately operated SQL Server test environment; Azure normally uses the system trust roots.

## Backups

Use [Azure SQL automated backups and point-in-time restore](https://learn.microsoft.com/en-us/azure/azure-sql/database/automated-backups-overview?view=azuresql) for the hosted database. Retention depends on your Azure configuration. **Those backups do not contain local photos or the Pi's configuration.**

Continue backing up `data_dir/photos` and the configuration/environment files separately and securely. For a coordinated checkpoint, stop the controller while making the photo backup and record the database restore time. Restore the database and a compatible photo backup together; an older photo backup may be missing images referenced by a newer database restore. The SQLite file is no longer the active journal after switching to Azure and is not an Azure backup.

Before restarting a restored controller, disable schedules and overrides if you do not intend them to resume immediately. Verify plants, associations, timestamps, image files, and equipment status before re-enabling automation.

## Tests

`cargo test --locked` runs the database abstraction and migration-copy tests against temporary SQLite stores, along with the controller/API suite. These tests do not prove Azure network or T-SQL compatibility.

A separate opt-in test exercises the real SQL Server backend. It requires a **dedicated, empty test database whose name ends in `_test`**, TLS trust, and the four Azure environment variables:

```sh
PLANT_AZURE_TEST=1 cargo test --locked --test azure_sql -- --ignored --nocapture
```

The live test writes test data and leaves it in that dedicated database for inspection. Recreate/empty the test database before rerunning. Never point it at your production journal.

## Seed inventory upgrade (schema version 2)

Before deploying the version with seed inventory, run the new binary with
`--migrate` using your existing Azure SQL configuration and migration identity
(as in the setup instructions above). This applies `0002_seed_inventory.sql`,
adds the `seeds` table, and retains existing journal data. It is safe to rerun:
applied versions are recorded in `dbo.journal_schema`. The cloud runtime keeps
`database.migrate = false` and the current application requires version 4 at startup. Stop older application instances
before the multi-user upgrade; they do not enforce garden isolation.

SQLite applies this migration automatically on startup. SQLite imports include
seed inventory when present; sources created before this feature import with
an empty inventory. Seed quantities are maintained manually, independently of
plant creation and journal entries.

## Manual photo uploads (schema version 3)

Run the new binary with `--migrate` before deploying to an Azure SQL runtime with
`database.migrate = false`. Migration `0003_seed_photos.sql` adds seed photo
associations and preserves existing records. SQLite applies it automatically.
Imports preserve seed photo links when present and accept older journals without
them. Uploaded image files remain in the local photos directory and must be
backed up with camera images.

## Accounts and shared gardens (schema version 4)

Migration `0004_gardens.sql` adds accounts, sessions, gardens, membership, garden
settings, and ownership on existing records. It reserves `ally.rippley@gmail.com`
as the original owner with no usable password. All existing journal data belongs
to that account's garden. Photo files stay in their current location; authenticated
image requests check garden membership.

Back up first, stop older web and Pi instances, run the new binary with `--migrate`
using the migration identity, then activate the owner with `--set-password
ally.rippley@gmail.com` in an interactive terminal using the same database config.
Start only the upgraded binaries. The runtime requires schema version 4 even when
`database.migrate = false`. See [account operations](accounts.md).

The legacy SQLite importer supports journals with just the original account and
garden, plus older journals without accounts. It refuses sources or destinations
with additional accounts/gardens. Use full database backup/restore for multi-user
installations; do not flatten them into one account through the legacy importer.
