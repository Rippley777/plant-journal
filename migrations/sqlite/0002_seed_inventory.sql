CREATE TABLE seeds (
 id TEXT NOT NULL PRIMARY KEY,
 name TEXT NOT NULL,
 variety TEXT NOT NULL DEFAULT '',
 quantity BIGINT NOT NULL CHECK(quantity BETWEEN 0 AND 1000000000),
 unit TEXT NOT NULL CHECK(unit IN ('seeds','packets')),
 supplier TEXT NOT NULL DEFAULT '',
 purchase_year BIGINT CHECK(purchase_year BETWEEN 1900 AND 2100),
 storage_location TEXT NOT NULL DEFAULT '',
 notes TEXT NOT NULL DEFAULT '',
 created_at BIGINT NOT NULL
);
