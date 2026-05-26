@echo off
setlocal

cd /d "%~dp0.."

where python >nul 2>nul
if errorlevel 1 (
    echo [setup_env] python was not found in PATH.
    echo [setup_env] Install Python 3.7+ and retry.
    exit /b 1
)

if not exist ".venv\Scripts\python.exe" (
    echo [setup_env] Creating .venv...
    python -m venv .venv
    if errorlevel 1 exit /b 1
)

echo [setup_env] Upgrading pip...
".venv\Scripts\python.exe" -m pip install --upgrade pip
if errorlevel 1 exit /b 1

echo [setup_env] Installing development and build dependencies...
".venv\Scripts\python.exe" -m pip install -e .[dev,build]
if errorlevel 1 exit /b 1

echo [setup_env] Environment is ready.
