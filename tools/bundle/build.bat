@echo off
setlocal

REM Build shim.exe + backend.exe into dist\python-bridge-mcp\

cd /d "%~dp0..\.."

if not exist ".venv\Scripts\python.exe" (
    echo [build] .venv is missing.
    echo [build] Run tools\setup_env.bat first.
    exit /b 1
)

".venv\Scripts\python.exe" tools\bundle\build.py %*
