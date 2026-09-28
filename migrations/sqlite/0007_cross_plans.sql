CREATE TABLE cross_plans (
 id TEXT PRIMARY KEY,
 garden_id TEXT NOT NULL REFERENCES gardens(id),
 name TEXT NOT NULL, name_key TEXT NOT NULL,
 species TEXT NOT NULL DEFAULT '', breeder TEXT NOT NULL DEFAULT '',
 notes TEXT NOT NULL DEFAULT '',
 parent_one_id TEXT NOT NULL, parent_two_id TEXT NOT NULL,
 converted_strain_id TEXT, converted_at INTEGER,
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 UNIQUE(garden_id,name_key),
 FOREIGN KEY(garden_id,parent_one_id) REFERENCES strains(garden_id,id),
 FOREIGN KEY(garden_id,parent_two_id) REFERENCES strains(garden_id,id),
 FOREIGN KEY(garden_id,converted_strain_id) REFERENCES strains(garden_id,id),
 CHECK((converted_strain_id IS NULL AND converted_at IS NULL) OR
       (converted_strain_id IS NOT NULL AND converted_at IS NOT NULL))
);
