-- Your SQL goes here
CREATE TABLE voices (
    id SERIAL PRIMARY KEY,
    voice_data BYTEA NOT NULL,
    qiita_id TEXT NOT NULL,
    title TEXT NOT NULL
)