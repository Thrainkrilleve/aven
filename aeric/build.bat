@echo off
REM AERIC orchestrator -- MSVC build. Run from a "x64 Native Tools" prompt,
REM or this script will locate and call vcvars64.bat for you.
setlocal
if "%VSCMD_VER%"=="" (
  for /f "usebackq delims=" %%i in (`
    "%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -find VC\Auxiliary\Build\vcvars64.bat 2^>nul
  `) do call "%%i" >nul
)
cl /nologo /W4 /O2 aeric.c aven_shim.c model_server_capacity.c main.c /Fe:aeric_demo.exe
if errorlevel 1 exit /b 1
echo(
aeric_demo.exe
endlocal
