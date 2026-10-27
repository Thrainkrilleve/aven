-- Live weather nowcast (/weather): one row per site per scheduled run, the forecasts each
-- run issues, and slowly changing source data (winds aloft, NWS points) kept between runs.
CREATE TABLE IF NOT EXISTS weather_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_ms INTEGER NOT NULL,
  site_kind TEXT NOT NULL,
  site_label TEXT NOT NULL,
  site_lat REAL NOT NULL,
  site_lon REAL NOT NULL,
  status TEXT NOT NULL,          -- ok | gated | error
  error TEXT,
  inputs TEXT,                   -- JSON: model inputs by name (null = not reported)
  outputs TEXT,                  -- JSON: model outputs by name
  sources TEXT,                  -- JSON: every request made for this run
  record TEXT,                   -- JSON: stations, cell, radar points, gauge, alerts, forecast
  model_sha256 TEXT
);
CREATE INDEX IF NOT EXISTS idx_weather_runs_created ON weather_runs (created_ms);

CREATE TABLE IF NOT EXISTS weather_forecasts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL,
  site_kind TEXT NOT NULL,
  lat REAL NOT NULL,
  lon REAL NOT NULL,
  lead_min INTEGER NOT NULL,
  method TEXT NOT NULL,          -- steering | cell_motion | persistence
  issued_ms INTEGER NOT NULL,    -- radar time the forecast is based on
  valid_ms INTEGER NOT NULL,
  predicted_dbz REAL NOT NULL,
  observed_dbz REAL,
  radar_ms INTEGER,              -- time of the radar image it was scored against
  verified_ms INTEGER,
  status TEXT NOT NULL DEFAULT 'pending'  -- pending | verified | expired
);
CREATE INDEX IF NOT EXISTS idx_weather_forecasts_status ON weather_forecasts (status, valid_ms);
CREATE INDEX IF NOT EXISTS idx_weather_forecasts_run ON weather_forecasts (run_id);

CREATE TABLE IF NOT EXISTS weather_cache (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  fetched_ms INTEGER NOT NULL
);
