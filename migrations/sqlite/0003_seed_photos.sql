CREATE TABLE photo_seeds (
 photo_id TEXT NOT NULL REFERENCES photos(id) ON DELETE CASCADE,
 seed_id TEXT NOT NULL REFERENCES seeds(id) ON DELETE CASCADE,
 PRIMARY KEY(photo_id,seed_id)
);
CREATE INDEX photo_seeds_seed ON photo_seeds(seed_id);
