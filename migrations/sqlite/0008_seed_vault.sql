ALTER TABLE seeds ADD breeder TEXT NOT NULL DEFAULT '';
ALTER TABLE seeds ADD acquired_on TEXT;
ALTER TABLE seeds ADD packet_code TEXT NOT NULL DEFAULT '';
CREATE UNIQUE INDEX seeds_garden_id ON seeds(garden_id,id);
CREATE TABLE germination_attempts (
 id TEXT PRIMARY KEY,
 garden_id TEXT NOT NULL REFERENCES gardens(id),
 seed_id TEXT NOT NULL,
 started_on TEXT NOT NULL,
 seeds_sown BIGINT NOT NULL CHECK(seeds_sown BETWEEN 1 AND 1000000000),
 seeds_germinated BIGINT CHECK(seeds_germinated BETWEEN 0 AND seeds_sown),
 notes TEXT NOT NULL DEFAULT '',
 created_at BIGINT NOT NULL,
 FOREIGN KEY(garden_id,seed_id) REFERENCES seeds(garden_id,id)
);
CREATE INDEX germination_seed ON germination_attempts(garden_id,seed_id,started_on);
ALTER TABLE plants ADD seed_id TEXT REFERENCES seeds(id);
ALTER TABLE plants ADD germination_id TEXT REFERENCES germination_attempts(id);
CREATE INDEX plants_seed ON plants(garden_id,seed_id);
CREATE INDEX plants_germination ON plants(garden_id,germination_id);
CREATE TRIGGER seeds_origin_update BEFORE UPDATE OF garden_id ON seeds WHEN
 EXISTS(SELECT 1 FROM plants WHERE seed_id=OLD.id AND garden_id<>NEW.garden_id)
 BEGIN SELECT RAISE(ABORT,'Packet has linked plants'); END;
CREATE TRIGGER plants_origin_insert BEFORE INSERT ON plants WHEN
 (NEW.seed_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM seeds WHERE id=NEW.seed_id AND garden_id=NEW.garden_id)) OR
 (NEW.germination_id IS NOT NULL AND (NEW.seed_id IS NULL OR NOT EXISTS(SELECT 1 FROM germination_attempts WHERE id=NEW.germination_id AND seed_id=NEW.seed_id AND garden_id=NEW.garden_id)))
 BEGIN SELECT RAISE(ABORT,'Invalid seed origin'); END;
CREATE TRIGGER plants_origin_update BEFORE UPDATE OF seed_id,germination_id,garden_id ON plants WHEN
 (NEW.seed_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM seeds WHERE id=NEW.seed_id AND garden_id=NEW.garden_id)) OR
 (NEW.germination_id IS NOT NULL AND (NEW.seed_id IS NULL OR NOT EXISTS(SELECT 1 FROM germination_attempts WHERE id=NEW.germination_id AND seed_id=NEW.seed_id AND garden_id=NEW.garden_id)))
 BEGIN SELECT RAISE(ABORT,'Invalid seed origin'); END;
CREATE TRIGGER germination_origin_update BEFORE UPDATE OF seed_id,garden_id ON germination_attempts WHEN
 EXISTS(SELECT 1 FROM plants WHERE germination_id=OLD.id AND (seed_id<>NEW.seed_id OR garden_id<>NEW.garden_id))
 BEGIN SELECT RAISE(ABORT,'Attempt has linked plants'); END;
