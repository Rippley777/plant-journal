CREATE TABLE garden_catalog_imports (
 garden_id TEXT NOT NULL REFERENCES gardens(id),
 catalog_id TEXT NOT NULL,
 imported_at BIGINT NOT NULL,
 PRIMARY KEY(garden_id,catalog_id)
);
-- Preserve previous imports, including deliberately deleted cannabis cards.
INSERT INTO garden_catalog_imports(garden_id,catalog_id,imported_at)
 SELECT garden_id,'cannabis',imported_at FROM strain_catalog_imports WHERE catalog_version>=2;
