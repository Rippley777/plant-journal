CREATE TABLE strains (
 id TEXT PRIMARY KEY, garden_id TEXT NOT NULL REFERENCES gardens(id),
 name TEXT NOT NULL, name_key TEXT NOT NULL, species TEXT NOT NULL DEFAULT '',
 breeder TEXT NOT NULL DEFAULT '', notes TEXT NOT NULL DEFAULT '',
 status TEXT NOT NULL DEFAULT 'unowned' CHECK(status IN ('unowned','wanted','collected')),
 parent_one_id TEXT, parent_two_id TEXT,
 lineage_note TEXT NOT NULL DEFAULT '', source_url TEXT NOT NULL DEFAULT '',
 created_at INTEGER NOT NULL,
 UNIQUE(garden_id,name_key), UNIQUE(garden_id,id),
 FOREIGN KEY(garden_id,parent_one_id) REFERENCES strains(garden_id,id),
 FOREIGN KEY(garden_id,parent_two_id) REFERENCES strains(garden_id,id),
 CHECK(parent_one_id IS NULL OR parent_one_id<>id),
 CHECK(parent_two_id IS NULL OR parent_two_id<>id)
);
ALTER TABLE plants ADD strain_id TEXT REFERENCES strains(id);
ALTER TABLE seeds ADD strain_id TEXT REFERENCES strains(id);
CREATE INDEX plants_strain ON plants(strain_id);
CREATE INDEX seeds_strain ON seeds(strain_id);
CREATE TABLE strain_catalog_imports (garden_id TEXT PRIMARY KEY REFERENCES gardens(id), imported_at INTEGER NOT NULL);
CREATE TRIGGER plants_strain_insert BEFORE INSERT ON plants WHEN NEW.strain_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM strains WHERE id=NEW.strain_id AND garden_id=NEW.garden_id) BEGIN SELECT RAISE(ABORT,'Strain belongs to another garden'); END;
CREATE TRIGGER plants_strain_update BEFORE UPDATE OF strain_id,garden_id ON plants WHEN NEW.strain_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM strains WHERE id=NEW.strain_id AND garden_id=NEW.garden_id) BEGIN SELECT RAISE(ABORT,'Strain belongs to another garden'); END;
CREATE TRIGGER seeds_strain_insert BEFORE INSERT ON seeds WHEN NEW.strain_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM strains WHERE id=NEW.strain_id AND garden_id=NEW.garden_id) BEGIN SELECT RAISE(ABORT,'Strain belongs to another garden'); END;
CREATE TRIGGER seeds_strain_update BEFORE UPDATE OF strain_id,garden_id ON seeds WHEN NEW.strain_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM strains WHERE id=NEW.strain_id AND garden_id=NEW.garden_id) BEGIN SELECT RAISE(ABORT,'Strain belongs to another garden'); END;
