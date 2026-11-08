# Host-side LIVE measured-input monitor for the models mesh (Windows).
# Watches each stage's real compute + hop time and reports how much of the
# observed token time they explain. Assumes `docker compose -f
# docker-compose.models.yml up` is running and `avenc` is on PATH (v0.9.0+).
#
#   ./watch-measured.ps1                      # default floor 0.1 tok/s (CPU-realistic)
#   ./watch-measured.ps1 -MinRate 0.8         # plan-target floor (will ALARM on CPU)
param(
  [string]$Url = "http://127.0.0.1:8080",
  [string]$Key = "dev-user-key",
  [int]$Interval = 3,
  [double]$MinRate = 0.1
)
$here  = Split-Path -Parent $MyInvocation.MyCommand.Path
$model = Join-Path $here "watch.aven"

Write-Host "Generating measured model from $Url/admin/status ..." -ForegroundColor Cyan
python (Join-Path $here "gen_route.py") --measured --from-url $Url --key $Key -o $model

# Relax the speed floor so CPU inference settles (raise with -MinRate for the plan target).
(Get-Content $model) -replace 'min_rate = [0-9.]+ token/s', "min_rate = $MinRate token/s" | Set-Content $model

Write-Host "Live measured monitor on $Url  (floor $MinRate tok/s, Ctrl+C to stop)" -ForegroundColor Green
# Run the poller->avenc pipe under cmd.exe: PowerShell's object pipeline buffers
# native-to-native pipes and won't stream line-by-line; cmd uses a real byte pipe.
$poller = Join-Path $here "..\isharded_poller.py"
$line = "set `"ISHARDED_URL=$Url`" & set `"ISHARDED_KEY=$Key`" & python `"$poller`" --measured --interval $Interval | avenc stream `"$model`""
cmd /c $line
