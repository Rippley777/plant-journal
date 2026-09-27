CREATE TABLE dbo.seeds (
 id nvarchar(36) NOT NULL PRIMARY KEY,
 name nvarchar(120) NOT NULL,
 variety nvarchar(160) NOT NULL DEFAULT '',
 quantity BIGINT NOT NULL CHECK(quantity BETWEEN 0 AND 1000000000),
 unit nvarchar(16) NOT NULL CHECK(unit IN ('seeds','packets')),
 supplier nvarchar(160) NOT NULL DEFAULT '',
 purchase_year BIGINT CHECK(purchase_year BETWEEN 1900 AND 2100),
 storage_location nvarchar(160) NOT NULL DEFAULT '',
 notes nvarchar(max) NOT NULL DEFAULT '',
 created_at BIGINT NOT NULL
);
