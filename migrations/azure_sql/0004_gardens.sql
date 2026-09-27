-- Existing records remain in the original owner's garden. The reserved account
-- cannot be claimed through signup; activate it with --set-password.
CREATE TABLE users (id nvarchar(36) PRIMARY KEY, email nvarchar(254) NOT NULL UNIQUE, password_hash nvarchar(256), created_at bigint NOT NULL);
CREATE TABLE gardens (id nvarchar(36) PRIMARY KEY, name nvarchar(256) NOT NULL, owner_id nvarchar(36) NOT NULL REFERENCES users(id));
CREATE TABLE garden_members (garden_id nvarchar(36) NOT NULL REFERENCES gardens(id), user_id nvarchar(36) NOT NULL REFERENCES users(id), PRIMARY KEY(garden_id,user_id));
CREATE TABLE sessions (token_hash nvarchar(256) PRIMARY KEY, user_id nvarchar(36) NOT NULL REFERENCES users(id), garden_id nvarchar(36) NOT NULL REFERENCES gardens(id), expires_at bigint NOT NULL);
CREATE INDEX sessions_expiry ON sessions(expires_at);
INSERT INTO users(id,email,created_at) VALUES('00000000-0000-0000-0000-000000000002','ally.rippley@gmail.com',0);
INSERT INTO gardens(id,name,owner_id) VALUES('00000000-0000-0000-0000-000000000001','Ally’s garden','00000000-0000-0000-0000-000000000002');
INSERT INTO garden_members(garden_id,user_id) VALUES('00000000-0000-0000-0000-000000000001','00000000-0000-0000-0000-000000000002');
ALTER TABLE plants ADD garden_id nvarchar(36) NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES gardens(id);
CREATE INDEX plants_garden ON plants(garden_id);
ALTER TABLE entries ADD garden_id nvarchar(36) NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES gardens(id);
CREATE INDEX entries_garden ON entries(garden_id);
ALTER TABLE photos ADD garden_id nvarchar(36) NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES gardens(id);
CREATE INDEX photos_garden ON photos(garden_id);
ALTER TABLE readings ADD garden_id nvarchar(36) NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES gardens(id);
CREATE INDEX readings_garden ON readings(garden_id);
ALTER TABLE devices ADD garden_id nvarchar(36) NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES gardens(id);
CREATE INDEX devices_garden ON devices(garden_id);
ALTER TABLE events ADD garden_id nvarchar(36) NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES gardens(id);
CREATE INDEX events_garden ON events(garden_id);
ALTER TABLE seeds ADD garden_id nvarchar(36) NOT NULL DEFAULT '00000000-0000-0000-0000-000000000001' REFERENCES gardens(id);
CREATE INDEX seeds_garden ON seeds(garden_id);
CREATE TABLE garden_settings (garden_id nvarchar(36) PRIMARY KEY REFERENCES gardens(id), timezone nvarchar(256) NOT NULL DEFAULT 'America/Chicago', photo_enabled bigint NOT NULL DEFAULT 0, photo_time nvarchar(256) NOT NULL DEFAULT '12:00');
INSERT INTO garden_settings(garden_id,timezone,photo_enabled,photo_time) SELECT '00000000-0000-0000-0000-000000000001',timezone,photo_enabled,photo_time FROM settings;
