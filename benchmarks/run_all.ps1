# run_all.ps1 -- build and run all four backends + the differential comparison.
# Usage:  powershell -File benchmarks\run_all.ps1 <scratchdir>
# Must find MSVC (vcvars64) and must be run from the repo root (F:\aven).
param([string]$Scratch = "$env:TEMP\aven_bench")

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $Scratch | Out-Null

if (-not $env:VSCMD_VER) {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  $vcvars = & $vswhere -latest -find VC\Auxiliary\Build\vcvars64.bat
  cmd /c "call `"$vcvars`" && set" | ForEach-Object { if ($_ -match '^(.*?)=(.*)$') { Set-Item "env:$($matches[1])" $matches[2] } }
}

Write-Host "== generate battery + oracle =="
python benchmarks\gen_and_oracle.py $Scratch

Write-Host "== build + run Aven-C (LTO) and C++ =="
cl /nologo /O2 /GL benchmarks\backend_c.c aeric\model_server_capacity.c /Fe:"$Scratch\bench_c.exe" /link /LTCG | Out-Null
cl /nologo /O2 /EHsc benchmarks\backend_cpp.cpp /Fe:"$Scratch\bench_cpp.exe" | Out-Null
& "$Scratch\bench_c.exe" $Scratch
& "$Scratch\bench_cpp.exe" $Scratch

Write-Host "== build + run Rust (AVEN evaluator) =="
cargo run --release --bin bench_aven -- $Scratch

Write-Host "== run Python =="
python benchmarks\backend_python.py $Scratch

Write-Host "== differential comparison =="
python benchmarks\compare.py $Scratch
