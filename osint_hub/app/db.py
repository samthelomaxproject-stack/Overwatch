import os
import sqlite3
from contextlib import contextmanager

DB_PATH = os.getenv("DB_PATH", os.path.expanduser("~/.config/overwatch/conflict_events.db"))

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

CREATE TABLE IF NOT EXISTS shodan_findings (
  id TEXT PRIMARY KEY,
  ip TEXT,
  port INTEGER,
  transport TEXT,
  org TEXT,
  isp TEXT,
  asn TEXT,
  hostnames TEXT,
  domains TEXT,
  product TEXT,
  version TEXT,
  os TEXT,
  tags TEXT,
  vulns TEXT,
  category TEXT,
  lat REAL,
  lon REAL,
  country_code TEXT,
  country_name TEXT,
  city TEXT,
  region_code TEXT,
  timestamp TEXT,
  last_seen TEXT,
  shodan_url TEXT,
  query TEXT,
  source TEXT DEFAULT 'shodan',
  region_key TEXT,
  inserted_at TEXT DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
  stale_score INTEGER DEFAULT 0
);

CREATE TABLE IF NOT EXISTS shodan_region_cache_state (
  region_key TEXT,
  category TEXT,
  last_discovery_at TEXT,
  ttl_sec INTEGER,
  last_result_count INTEGER,
  last_status TEXT,
  last_error TEXT,
  PRIMARY KEY(region_key, category)
);

CREATE TABLE IF NOT EXISTS shodan_query_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  query TEXT,
  bbox TEXT,
  country TEXT,
  region_key TEXT,
  category TEXT,
  limit_requested INTEGER,
  result_count INTEGER,
  started_at TEXT,
  finished_at TEXT,
  status TEXT,
  error TEXT
);

CREATE TABLE IF NOT EXISTS shodan_credit_usage (
  date TEXT PRIMARY KEY,
  queries_used INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_shodan_updated ON shodan_findings(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_shodan_category ON shodan_findings(category);
CREATE INDEX IF NOT EXISTS idx_shodan_geo ON shodan_findings(lat, lon);
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
