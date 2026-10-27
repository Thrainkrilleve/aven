-- The NWS hourly forecast's chance of precipitation (%) for the hour each forecast falls
-- in, so the official forecast is scored against the same radar outcome as Aven's.
ALTER TABLE weather_forecasts ADD COLUMN nws_pop_pct REAL;
