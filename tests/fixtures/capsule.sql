-- Synthetic fixture, adapted from Capsule entries.rs tests; no personal data.
CREATE TABLE entries (
 id INTEGER PRIMARY KEY AUTOINCREMENT, uuid TEXT UNIQUE,
 created_at TEXT NOT NULL, updated_at TEXT, text TEXT NOT NULL,
 text_plain TEXT NOT NULL DEFAULT '', content_format TEXT NOT NULL DEFAULT 'plain',
 title TEXT, summary TEXT, mood TEXT, starred INTEGER DEFAULT 0,
 pinned INTEGER DEFAULT 0, hidden INTEGER DEFAULT 0
);
CREATE TABLE tags (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE);
CREATE TABLE entry_tags (entry_id INTEGER NOT NULL, tag_id INTEGER NOT NULL, PRIMARY KEY(entry_id, tag_id));
CREATE VIRTUAL TABLE entries_fts USING fts5(text);
CREATE TABLE entry_continuations (child_entry_uuid TEXT PRIMARY KEY, parent_entry_uuid TEXT NOT NULL, updated_at TEXT);
CREATE TABLE entry_thread_titles (thread_root_uuid TEXT PRIMARY KEY, title TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE entry_thread_summaries (thread_root_uuid TEXT PRIMARY KEY, summary TEXT NOT NULL, updated_at TEXT NOT NULL);
INSERT INTO entries (uuid, created_at, updated_at, text, text_plain, mood) VALUES
 ('entry_fixture1','2026-09-13 09:00','2026-09-13 09:00','A synthetic garden walk.','A synthetic garden walk.','content');
INSERT INTO entries (uuid, created_at, updated_at, text, text_plain, hidden) VALUES
 ('entry_hidden01','2026-09-14 10:00','2026-09-14 10:00','A hidden synthetic note.','A hidden synthetic note.',1);
INSERT INTO tags (name) VALUES ('outdoors');
INSERT INTO entry_tags VALUES (1,1);
INSERT INTO entries_fts (rowid,text) SELECT id,text_plain FROM entries;
