CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS terms (
  id TEXT PRIMARY KEY NOT NULL,
  source TEXT NOT NULL,
  target TEXT NOT NULL,
  note TEXT NOT NULL,
  sort_order INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS hotword_groups (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  sort_order INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS hotword_terms (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  group_id TEXT NOT NULL,
  term TEXT NOT NULL,
  sort_order INTEGER NOT NULL,
  FOREIGN KEY(group_id) REFERENCES hotword_groups(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS hotword_meta (
  singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
  enabled INTEGER NOT NULL,
  active_group_id TEXT NOT NULL
);
