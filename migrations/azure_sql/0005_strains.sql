CREATE TABLE strains (
 id nvarchar(36) PRIMARY KEY, garden_id nvarchar(36) NOT NULL REFERENCES gardens(id),
 name nvarchar(120) NOT NULL, name_key nvarchar(240) NOT NULL,
 species nvarchar(160) NOT NULL DEFAULT '', breeder nvarchar(160) NOT NULL DEFAULT '',
 notes nvarchar(max) NOT NULL DEFAULT '',
 status nvarchar(16) NOT NULL DEFAULT 'unowned' CHECK(status IN ('unowned','wanted','collected')),
 parent_one_id nvarchar(36), parent_two_id nvarchar(36),
 lineage_note nvarchar(2000) NOT NULL DEFAULT '', source_url nvarchar(1000) NOT NULL DEFAULT '',
 created_at bigint NOT NULL,
 UNIQUE(garden_id,name_key), UNIQUE(garden_id,id),
 FOREIGN KEY(garden_id,parent_one_id) REFERENCES strains(garden_id,id),
 FOREIGN KEY(garden_id,parent_two_id) REFERENCES strains(garden_id,id),
 CHECK(parent_one_id IS NULL OR parent_one_id<>id),
 CHECK(parent_two_id IS NULL OR parent_two_id<>id)
);
ALTER TABLE plants ADD strain_id nvarchar(36);
ALTER TABLE seeds ADD strain_id nvarchar(36);
ALTER TABLE plants ADD CONSTRAINT plants_strain_garden FOREIGN KEY(garden_id,strain_id) REFERENCES strains(garden_id,id);
ALTER TABLE seeds ADD CONSTRAINT seeds_strain_garden FOREIGN KEY(garden_id,strain_id) REFERENCES strains(garden_id,id);
CREATE INDEX plants_strain ON plants(strain_id);
CREATE INDEX seeds_strain ON seeds(strain_id);
CREATE TABLE strain_catalog_imports (garden_id nvarchar(36) PRIMARY KEY REFERENCES gardens(id), imported_at bigint NOT NULL);
