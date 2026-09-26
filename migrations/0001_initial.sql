CREATE TABLE plants (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, species TEXT NOT NULL DEFAULT '',
 notes TEXT NOT NULL DEFAULT '', archived INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL
);
CREATE TABLE entries (
 id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('note','watering','feeding','pruning','repotting')),
 body TEXT NOT NULL, occurred_at INTEGER NOT NULL, created_at INTEGER NOT NULL
);
CREATE TABLE entry_plants (
 entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
 plant_id TEXT NOT NULL REFERENCES plants(id), PRIMARY KEY(entry_id,plant_id)
);
CREATE TABLE photos (
 id TEXT PRIMARY KEY, filename TEXT NOT NULL UNIQUE, captured_at INTEGER NOT NULL, source TEXT NOT NULL
);
CREATE TABLE photo_plants (
 photo_id TEXT NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
 plant_id TEXT NOT NULL REFERENCES plants(id), PRIMARY KEY(photo_id,plant_id)
);
CREATE TABLE readings (
 id INTEGER PRIMARY KEY, recorded_at INTEGER NOT NULL, temperature_c REAL NOT NULL, humidity_percent REAL NOT NULL
);
CREATE INDEX readings_time ON readings(recorded_at);
CREATE TABLE devices (
 id TEXT PRIMARY KEY, name TEXT NOT NULL, role TEXT NOT NULL CHECK(role IN ('light','fan')),
 adapter TEXT NOT NULL CHECK(adapter IN ('simulated','shelly')), address TEXT NOT NULL DEFAULT '',
 channel INTEGER NOT NULL DEFAULT 0, commanded_on INTEGER, reported_on INTEGER,
 checked_at INTEGER, last_error TEXT
);
CREATE TABLE schedules (
 device_id TEXT PRIMARY KEY REFERENCES devices(id), enabled INTEGER NOT NULL DEFAULT 0,
 start_time TEXT NOT NULL DEFAULT '08:00', end_time TEXT NOT NULL DEFAULT '20:00'
);
CREATE TABLE overrides (
 device_id TEXT PRIMARY KEY REFERENCES devices(id), on_state INTEGER NOT NULL, expires_at INTEGER NOT NULL
);
CREATE TABLE events (
 id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL, occurred_at INTEGER NOT NULL,
 entity_id TEXT, detail TEXT NOT NULL DEFAULT ''
);
CREATE INDEX events_time ON events(occurred_at);
CREATE TABLE event_plants (
 event_id TEXT NOT NULL REFERENCES events(id) ON DELETE CASCADE,
 plant_id TEXT NOT NULL REFERENCES plants(id), PRIMARY KEY(event_id,plant_id)
);
CREATE TABLE capture_runs (
 local_date TEXT PRIMARY KEY, status TEXT NOT NULL, attempted_at INTEGER NOT NULL
);
CREATE TABLE settings (
 id INTEGER PRIMARY KEY CHECK(id = 1), timezone TEXT NOT NULL DEFAULT 'America/Chicago',
 photo_enabled INTEGER NOT NULL DEFAULT 0, photo_time TEXT NOT NULL DEFAULT '12:00'
);
INSERT INTO settings(id) VALUES(1);
CREATE TABLE health (
 component TEXT PRIMARY KEY, last_success INTEGER, last_error TEXT, checked_at INTEGER NOT NULL
);
