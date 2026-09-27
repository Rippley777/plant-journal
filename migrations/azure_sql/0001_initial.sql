-- All timestamps are Unix seconds (bigint), preserving the application's API and timezone semantics.
CREATE TABLE dbo.plants (
 id nvarchar(36) NOT NULL PRIMARY KEY, name nvarchar(120) NOT NULL, species nvarchar(160) NOT NULL DEFAULT N'',
 notes nvarchar(max) NOT NULL DEFAULT N'', archived bit NOT NULL DEFAULT 0, created_at bigint NOT NULL
);
CREATE TABLE dbo.entries (
 id nvarchar(36) NOT NULL PRIMARY KEY, kind nvarchar(32) NOT NULL CHECK(kind IN ('note','watering','feeding','pruning','repotting')),
 body nvarchar(max) NOT NULL, occurred_at bigint NOT NULL, created_at bigint NOT NULL
);
CREATE TABLE dbo.entry_plants (
 entry_id nvarchar(36) NOT NULL REFERENCES dbo.entries(id) ON DELETE CASCADE,
 plant_id nvarchar(36) NOT NULL REFERENCES dbo.plants(id), PRIMARY KEY(entry_id,plant_id)
);
CREATE TABLE dbo.photos (
 id nvarchar(36) NOT NULL PRIMARY KEY, filename nvarchar(80) NOT NULL UNIQUE, captured_at bigint NOT NULL, source nvarchar(32) NOT NULL
);
CREATE TABLE dbo.photo_plants (
 photo_id nvarchar(36) NOT NULL REFERENCES dbo.photos(id) ON DELETE CASCADE,
 plant_id nvarchar(36) NOT NULL REFERENCES dbo.plants(id), PRIMARY KEY(photo_id,plant_id)
);
CREATE TABLE dbo.readings (
 id bigint IDENTITY(1,1) NOT NULL PRIMARY KEY, recorded_at bigint NOT NULL, temperature_c float NOT NULL, humidity_percent float NOT NULL
);
CREATE INDEX readings_time ON dbo.readings(recorded_at);
CREATE TABLE dbo.devices (
 id nvarchar(36) NOT NULL PRIMARY KEY, name nvarchar(120) NOT NULL, role nvarchar(16) NOT NULL CHECK(role IN ('light','fan')),
 adapter nvarchar(16) NOT NULL CHECK(adapter IN ('simulated','shelly')), address nvarchar(2048) NOT NULL DEFAULT N'',
 channel bigint NOT NULL DEFAULT 0, commanded_on bit NULL, reported_on bit NULL,
 checked_at bigint NULL, last_error nvarchar(max) NULL
);
CREATE TABLE dbo.schedules (
 device_id nvarchar(36) NOT NULL PRIMARY KEY REFERENCES dbo.devices(id), enabled bit NOT NULL DEFAULT 0,
 start_time nvarchar(5) NOT NULL DEFAULT N'08:00', end_time nvarchar(5) NOT NULL DEFAULT N'20:00'
);
CREATE TABLE dbo.overrides (
 device_id nvarchar(36) NOT NULL PRIMARY KEY REFERENCES dbo.devices(id), on_state bit NOT NULL, expires_at bigint NOT NULL
);
CREATE TABLE dbo.events (
 id nvarchar(36) NOT NULL PRIMARY KEY, kind nvarchar(32) NOT NULL, title nvarchar(256) NOT NULL, occurred_at bigint NOT NULL,
 entity_id nvarchar(36) NULL, detail nvarchar(max) NOT NULL DEFAULT N''
);
CREATE INDEX events_time ON dbo.events(occurred_at);
CREATE TABLE dbo.event_plants (
 event_id nvarchar(36) NOT NULL REFERENCES dbo.events(id) ON DELETE CASCADE,
 plant_id nvarchar(36) NOT NULL REFERENCES dbo.plants(id), PRIMARY KEY(event_id,plant_id)
);
CREATE TABLE dbo.capture_runs (
 local_date nvarchar(10) NOT NULL PRIMARY KEY, status nvarchar(32) NOT NULL, attempted_at bigint NOT NULL
);
CREATE TABLE dbo.settings (
 id bigint NOT NULL PRIMARY KEY CHECK(id=1), timezone nvarchar(128) NOT NULL DEFAULT N'America/Chicago',
 photo_enabled bit NOT NULL DEFAULT 0, photo_time nvarchar(5) NOT NULL DEFAULT N'12:00'
);
INSERT INTO dbo.settings(id) VALUES(1);
CREATE TABLE dbo.health (
 component nvarchar(32) NOT NULL PRIMARY KEY, last_success bigint NULL, last_error nvarchar(max) NULL, checked_at bigint NOT NULL
);
