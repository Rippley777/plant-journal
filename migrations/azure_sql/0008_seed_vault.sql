ALTER TABLE seeds ADD breeder nvarchar(160) NOT NULL DEFAULT '', acquired_on nvarchar(10), packet_code nvarchar(160) NOT NULL DEFAULT '';
ALTER TABLE seeds ADD CONSTRAINT seeds_garden_id UNIQUE(garden_id,id);
CREATE TABLE germination_attempts (
 id nvarchar(36) PRIMARY KEY,
 garden_id nvarchar(36) NOT NULL REFERENCES gardens(id),
 seed_id nvarchar(36) NOT NULL,
 started_on nvarchar(10) NOT NULL,
 seeds_sown bigint NOT NULL CHECK(seeds_sown BETWEEN 1 AND 1000000000),
 seeds_germinated bigint,
 notes nvarchar(max) NOT NULL DEFAULT '',
 created_at bigint NOT NULL,
 CHECK(seeds_germinated BETWEEN 0 AND seeds_sown),
 UNIQUE(garden_id,seed_id,id),
 FOREIGN KEY(garden_id,seed_id) REFERENCES seeds(garden_id,id)
);
CREATE INDEX germination_seed ON germination_attempts(garden_id,seed_id,started_on);
ALTER TABLE plants ADD seed_id nvarchar(36), germination_id nvarchar(36);
ALTER TABLE plants ADD CONSTRAINT plants_seed_origin FOREIGN KEY(garden_id,seed_id) REFERENCES seeds(garden_id,id);
ALTER TABLE plants ADD CONSTRAINT plants_germination_origin FOREIGN KEY(garden_id,seed_id,germination_id) REFERENCES germination_attempts(garden_id,seed_id,id);
-- Compile this expression after the new columns exist in the enclosing batch.
EXEC(N'ALTER TABLE plants ADD CONSTRAINT plants_germination_packet CHECK(germination_id IS NULL OR seed_id IS NOT NULL)');
CREATE INDEX plants_seed ON plants(garden_id,seed_id);
CREATE INDEX plants_germination ON plants(garden_id,germination_id);
