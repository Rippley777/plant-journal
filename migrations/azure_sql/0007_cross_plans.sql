CREATE TABLE cross_plans (
 id nvarchar(36) PRIMARY KEY,
 garden_id nvarchar(36) NOT NULL REFERENCES gardens(id),
 name nvarchar(120) NOT NULL, name_key nvarchar(240) NOT NULL,
 species nvarchar(160) NOT NULL DEFAULT '', breeder nvarchar(160) NOT NULL DEFAULT '',
 notes nvarchar(max) NOT NULL DEFAULT '',
 parent_one_id nvarchar(36) NOT NULL, parent_two_id nvarchar(36) NOT NULL,
 converted_strain_id nvarchar(36), converted_at bigint,
 created_at bigint NOT NULL, updated_at bigint NOT NULL,
 UNIQUE(garden_id,name_key),
 FOREIGN KEY(garden_id,parent_one_id) REFERENCES strains(garden_id,id),
 FOREIGN KEY(garden_id,parent_two_id) REFERENCES strains(garden_id,id),
 FOREIGN KEY(garden_id,converted_strain_id) REFERENCES strains(garden_id,id),
 CHECK((converted_strain_id IS NULL AND converted_at IS NULL) OR
       (converted_strain_id IS NOT NULL AND converted_at IS NOT NULL))
);
