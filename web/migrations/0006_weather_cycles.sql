-- One row per scheduled task of the weather nowcast. Written when the task starts and
-- completed when it ends: a row with no finished_ms is a task the platform stopped
-- (CPU or time limit) before it could finish.
CREATE TABLE IF NOT EXISTS weather_cycles (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  started_ms INTEGER NOT NULL,
  task TEXT NOT NULL,
  finished_ms INTEGER,
  sites INTEGER,
  verified INTEGER,
  requests INTEGER,
  error TEXT
);
CREATE INDEX IF NOT EXISTS idx_weather_cycles_started ON weather_cycles (started_ms);
