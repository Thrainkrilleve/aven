# Host-side live monitor for the stage-1 mesh (Windows).
# Assumes `docker compose up` is running and avenc.exe is built.
param(
  [string]$Url = "http://127.0.0.1:8080",
  [string]$Key = "dev-user-key",
  [string]$Avenc = "..\..\..\..\target\release\avenc.exe",
  [int]$Interval = 2
)
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$model = Join-Path $here "route.aven"

Write-Host "Generating route model from $Url/admin/status ..."
python (Join-Path $here "gen_route.py") --from-url $Url --key $Key -o $model

Write-Host "Starting live monitor (Ctrl+C to stop) ..."
$env:ISHARDED_URL = $Url
$env:ISHARDED_KEY = $Key
python (Join-Path $here "..\isharded_poller.py") --interval $Interval | & $Avenc stream $model
