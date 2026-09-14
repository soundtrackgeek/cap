-- Canonical normal fixture, adapted from Capsule's entries/location/images tests.
-- All values are synthetic; this file must never be replaced with a journal export.
CREATE TABLE entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid TEXT UNIQUE,
    created_at TEXT NOT NULL,
    updated_at TEXT,
    text TEXT NOT NULL,
    text_plain TEXT NOT NULL DEFAULT '',
    content_format TEXT NOT NULL DEFAULT 'plain',
    title TEXT,
    summary TEXT,
    mood TEXT,
    starred INTEGER DEFAULT 0,
    pinned INTEGER DEFAULT 0,
    hidden INTEGER DEFAULT 0
);
CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE
);
CREATE TABLE entry_tags (
    entry_id INTEGER NOT NULL,
    tag_id INTEGER NOT NULL,
    PRIMARY KEY (entry_id, tag_id)
);
CREATE TABLE entry_continuations (
    child_entry_uuid TEXT PRIMARY KEY,
    parent_entry_uuid TEXT NOT NULL,
    updated_at TEXT
);
CREATE TABLE entry_thread_titles (
    thread_root_uuid TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE entry_thread_summaries (
    thread_root_uuid TEXT PRIMARY KEY,
    summary TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    operation_type TEXT NOT NULL,
    entry_id INTEGER NOT NULL,
    old_data TEXT NOT NULL,
    additional_data TEXT,
    undone INTEGER DEFAULT 0,
    redo_data TEXT
);
CREATE TABLE plugin_media_assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT NOT NULL UNIQUE,
    mime_type TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    width INTEGER NOT NULL,
    height INTEGER NOT NULL,
    storage_backend TEXT NOT NULL,
    storage_key TEXT NOT NULL,
    created_at TEXT NOT NULL,
    deleted_at TEXT
);
CREATE TABLE plugin_entry_media (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_uuid TEXT NOT NULL,
    media_id INTEGER NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    caption TEXT,
    alt_text TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (entry_uuid) REFERENCES entries(uuid) ON DELETE CASCADE,
    FOREIGN KEY (media_id) REFERENCES plugin_media_assets(id) ON DELETE CASCADE
);
CREATE TABLE plugin_entry_locations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_uuid TEXT NOT NULL UNIQUE,
    latitude REAL NOT NULL,
    longitude REAL NOT NULL,
    place_name TEXT,
    place_details TEXT,
    source TEXT NOT NULL DEFAULT 'auto',
    weather_condition TEXT,
    weather_temp_c REAL,
    weather_temp_f REAL,
    weather_icon TEXT,
    weather_humidity INTEGER,
    weather_wind_kph REAL,
    weather_fetched_at TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (entry_uuid) REFERENCES entries(uuid) ON DELETE CASCADE
);
CREATE TABLE plugin_location_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    latitude REAL NOT NULL,
    longitude REAL NOT NULL,
    place_name TEXT NOT NULL,
    place_details TEXT,
    reverse_geocoded_at TEXT NOT NULL,
    UNIQUE (latitude, longitude)
);
CREATE TABLE sync_location_tombstones (
    entry_uuid TEXT NOT NULL PRIMARY KEY,
    deleted_at TEXT NOT NULL
);
CREATE TABLE sync_entry_thread_title_tombstones (
    thread_root_uuid TEXT PRIMARY KEY,
    deleted_at TEXT NOT NULL
);
CREATE TABLE sync_entry_thread_summary_tombstones (
    thread_root_uuid TEXT PRIMARY KEY,
    deleted_at TEXT NOT NULL
);
CREATE TABLE sync_image_tombstones (
    entry_uuid TEXT NOT NULL,
    asset_hash TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    caption TEXT,
    alt_text TEXT,
    deleted_at TEXT NOT NULL,
    PRIMARY KEY (entry_uuid, asset_hash, position, caption, alt_text)
);
CREATE VIRTUAL TABLE entries_fts USING fts5(text);

INSERT INTO entries
    (uuid, created_at, updated_at, text, text_plain, content_format,
     title, summary, mood, starred, pinned, hidden)
VALUES
    ('entry_root', '2026-09-10 08:00', '2026-09-10 08:00',
     'Root text for the synthetic garden walk.',
     'Root text for the synthetic garden walk.', 'markdown',
     'Root', 'Thread root', 'happy', 1, 0, 0),
    ('entry_middle', '2026-09-11 08:00', '2026-09-11 08:00',
     'Middle text continues the walk.', 'Middle text continues the walk.',
     'plain', NULL, NULL, 'calm', 0, 0, 0),
    ('entry_child', '2026-09-12 08:00', '2026-09-12 08:00',
     'Child text records the weather stop.',
     'Child text records the weather stop.', 'plain',
     'Child', 'Weather stop', 'focused', 0, 1, 0),
    ('entry_hidden', '2026-09-13 10:00', '2026-09-13 10:00',
     'Hidden text is excluded from normal reads.',
     'Hidden text is excluded from normal reads.', 'plain',
     NULL, NULL, 'quiet', 0, 0, 1),
    ('entry_today', '2026-09-14 09:00', '2026-09-14 09:00',
     'Today is a visible follow-up note.',
     'Today is a visible follow-up note.', 'markdown',
     'Today', 'Latest note', NULL, 0, 0, 0);

INSERT INTO tags (name) VALUES ('personal'), ('work'), ('outdoors');
INSERT INTO entry_tags (entry_id, tag_id)
VALUES (1, 1), (2, 3), (3, 2), (5, 3);
INSERT INTO entry_continuations (child_entry_uuid, parent_entry_uuid, updated_at)
VALUES ('entry_middle', 'entry_root', '2026-09-11 08:00'),
       ('entry_child', 'entry_middle', '2026-09-12 08:00'),
       ('entry_today', 'entry_child', '2026-09-14 09:00');
INSERT INTO entry_thread_titles (thread_root_uuid, title, updated_at)
VALUES ('entry_root', 'Thread title', '2026-09-14 09:00');
INSERT INTO entry_thread_summaries (thread_root_uuid, summary, updated_at)
VALUES ('entry_root', 'Thread summary', '2026-09-14 09:00');
INSERT INTO history
    (timestamp, operation_type, entry_id, old_data, additional_data, undone)
VALUES ('2026-09-12 08:01', 'EDIT_TEXT', 3, '{}', '{"source":"fixture"}', 0);
INSERT INTO plugin_media_assets
    (hash, mime_type, bytes, width, height, storage_backend, storage_key, created_at)
VALUES ('asset-one', 'image/jpeg', 100, 400, 300, 'local_fs',
        'fixture/asset-one.jpg', '2026-09-12 08:00'),
       ('asset-two', 'image/jpeg', 120, 400, 300, 'local_fs',
        'fixture/asset-two.jpg', '2026-09-12 08:00');
INSERT INTO plugin_entry_media
    (entry_uuid, media_id, position, caption, alt_text, created_at)
VALUES ('entry_child', 1, 0, 'A synthetic image', 'Synthetic image', '2026-09-12 08:00'),
       ('entry_today', 2, 0, NULL, 'Another synthetic image', '2026-09-14 09:00');
INSERT INTO plugin_entry_locations
    (entry_uuid, latitude, longitude, place_name, place_details, source,
     weather_condition, weather_temp_c, weather_temp_f, weather_icon,
     weather_humidity, weather_wind_kph, weather_fetched_at, created_at)
VALUES ('entry_child', 69.65, 18.96, 'Tromso', 'Synthetic harbor', 'manual',
        'Overcast', 8.0, 46.4, 'cloudy', 82, 11.4,
        '2026-09-12 08:05', '2026-09-12 08:00'),
       ('entry_today', 60.39, 5.32, 'Bergen', 'Synthetic quay', 'default',
        'Light rain', 12.0, 53.6, 'rain', 78, 8.2,
        '2026-09-14 09:05', '2026-09-14 09:00');
INSERT INTO plugin_location_cache
    (latitude, longitude, place_name, place_details, reverse_geocoded_at)
VALUES (60.39, 5.32, 'Bergen', 'Synthetic quay', '2026-09-14 09:04');
INSERT INTO sync_location_tombstones (entry_uuid, deleted_at)
VALUES ('entry_deleted_fixture', '2026-09-13 10:00');
INSERT INTO sync_entry_thread_title_tombstones (thread_root_uuid, deleted_at)
VALUES ('entry_old_thread', '2026-09-13 10:00');
INSERT INTO sync_entry_thread_summary_tombstones (thread_root_uuid, deleted_at)
VALUES ('entry_old_thread', '2026-09-13 10:00');
INSERT INTO sync_image_tombstones
    (entry_uuid, asset_hash, position, caption, alt_text, deleted_at)
VALUES ('entry_deleted_fixture', 'asset-old', 0, NULL, 'Old synthetic image', '2026-09-13 10:00');
INSERT INTO entries_fts (rowid, text)
SELECT rowid, text_plain FROM entries;
