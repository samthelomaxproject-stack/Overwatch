import os
import sqlite3
from contextlib import contextmanager

DB_PATH = os.getenv("DB_PATH", "./conflict_events.db")

SCHEMA_SQL = """
CREATE TABLE IF NOT EXISTS conflict_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  external_id TEXT UNIQUE,
  source_system TEXT NOT NULL,
  event_date TEXT NOT NULL,
  country TEXT,
  admin1 TEXT,
  location TEXT,
  latitude REAL NOT NULL,
  longitude REAL NOT NULL,
  event_type TEXT NOT NULL,
  sub_event_type TEXT,
  actor1 TEXT,
  actor2 TEXT,
  fatalities INTEGER,
  notes TEXT,
  source_scale TEXT,
  confidence_score REAL,
  created_at TEXT DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS event_sources (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  event_id INTEGER NOT NULL,
  source_name TEXT NOT NULL,
  source_url TEXT,
  source_type TEXT,
  is_primary INTEGER DEFAULT 0,
  FOREIGN KEY(event_id) REFERENCES conflict_events(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_events_date ON conflict_events(event_date DESC);
CREATE INDEX IF NOT EXISTS idx_events_type ON conflict_events(event_type);
CREATE INDEX IF NOT EXISTS idx_events_country ON conflict_events(country);
"""


def init_db():
    with sqlite3.connect(DB_PATH) as conn:
        conn.executescript(SCHEMA_SQL)
        conn.commit()


@contextmanager
def get_conn():
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    try:
        yield conn
    finally:
        conn.close()
