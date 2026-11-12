@echo off
setlocal enabledelayedexpansion
title OpenIDM - Build Standalone EXE
chcp 65001 >nul
color 0B
cd /d "%~dp0"

echo =========================================================================
echo             OPENIDM - BUILD STANDALONE PORTABLE EXECUTABLE
echo =========================================================================
echo.

:: 1. Deteksi Python
set "PY_CMD="
py -c "import sys; sys.exit(0)" >nul 2>&1
if not errorlevel 1 set "PY_CMD=py"

if "%PY_CMD%"=="" (
    python -c "import sys; sys.exit(0)" >nul 2>&1
    if not errorlevel 1 set "PY_CMD=python"
)

if "%PY_CMD%"=="" (
    if exist "%LOCALAPPDATA%\Python\pythoncore-3.14-64\python.exe" (
        set "PY_CMD=%LOCALAPPDATA%\Python\pythoncore-3.14-64\python.exe"
    )
)

if "%PY_CMD%"=="" (
    color 0C
    echo [ERROR] Python tidak ditemukan di sistem!
    pause
    exit /b 1
)

echo [*] Menggunakan Python: %PY_CMD%

:: 2. Pastikan PyInstaller terpasang
"%PY_CMD%" -c "import PyInstaller" >nul 2>&1
if errorlevel 1 (
    echo [*] Menginstall PyInstaller...
    "%PY_CMD%" -m pip install -q pyinstaller
)

:: 3. Jalankan Skrip Builder
echo [*] Memulai proses kompilasi...
"%PY_CMD%" scripts\build_exe.py

if errorlevel 1 (
    echo.
    color 0C
    echo [!] Build gagal!
    pause
    exit /b 1
)

echo.
echo =========================================================================
echo  BUILD SUKSES! File OpenIDM.exe berada di folder dist\OpenIDM\
echo =========================================================================
echo.
pause

