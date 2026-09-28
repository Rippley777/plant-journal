CREATE TABLE garden_catalog_imports (
 garden_id nvarchar(36) NOT NULL REFERENCES gardens(id),
 catalog_id nvarchar(40) NOT NULL,
 imported_at bigint NOT NULL,
 PRIMARY KEY(garden_id,catalog_id)
);
INSERT INTO garden_catalog_imports(garden_id,catalog_id,imported_at)
 SELECT garden_id,'cannabis',imported_at FROM strain_catalog_imports WHERE catalog_version>=2;
