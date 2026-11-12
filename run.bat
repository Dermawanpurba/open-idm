@echo off
setlocal enabledelayedexpansion
title OpenIDM - Desktop Core ^& Dashboard
chcp 65001 >nul
color 0B
cd /d "%~dp0"

echo =========================================================================
echo             OPENIDM - OPEN SOURCE DOWNLOAD MANAGER ^& CORE
echo =========================================================================
echo.

:: 1. Deteksi Python yang benar-benar aktif (mengabaikan alias dummy Microsoft Store)
set "PY_CMD="

:: Prioritas 1: py launcher (Windows Python Launcher resmi)
py -c "import sys; sys.exit(0)" >nul 2>&1
if not errorlevel 1 (
    set "PY_CMD=py"
    goto :py_ready
)

:: Prioritas 2: python langsung dari PATH (harus bisa run tanpa error)
python -c "import sys; sys.exit(0)" >nul 2>&1
if not errorlevel 1 (
    set "PY_CMD=python"
    goto :py_ready
)

:: Prioritas 3: Deteksi instalasi Python di Local AppData
if exist "%LOCALAPPDATA%\Python\pythoncore-3.14-64\python.exe" (
    "%LOCALAPPDATA%\Python\pythoncore-3.14-64\python.exe" -c "import sys; sys.exit(0)" >nul 2>&1
    if not errorlevel 1 (
        set "PY_CMD=%LOCALAPPDATA%\Python\pythoncore-3.14-64\python.exe"
        goto :py_ready
    )
)

:: Prioritas 4: Loop scan instalasi Python lainnya
for /d %%D in ("%LOCALAPPDATA%\Programs\Python\Python*" "%LOCALAPPDATA%\Python\pythoncore*") do (
    if exist "%%D\python.exe" (
        "%%D\python.exe" -c "import sys; sys.exit(0)" >nul 2>&1
        if not errorlevel 1 (
            set "PY_CMD=%%D\python.exe"
            goto :py_ready
        )
    )
)

:py_ready
if "%PY_CMD%"=="" (
    color 0C
    echo [ERROR] Python tidak ditemukan di sistem!
    echo Silakan install Python 3.10+ dari https://www.python.org/
    echo dan pastikan centang opsi "Add Python to PATH".
    echo.
    pause
    exit /b 1
)

echo [*] Python terdeteksi: %PY_CMD%

:: 2. Bebaskan Port 6899 dari proses zombie/lama jika ada
for /f "tokens=5" %%a in ('netstat -aon ^| findstr ":6899" ^| findstr "LISTENING"') do (
    if not "%%a"=="" if not "%%a"=="0" (
        echo [!] Membebaskan port 6899 dari proses lama PID %%a...
        taskkill /F /PID %%a >nul 2>&1
    )
)

:: 3. Verifikasi dependensi pip penting
"%PY_CMD%" -c "import requests, yt_dlp" >nul 2>&1
if errorlevel 1 (
    echo [*] Menginstall/memperbarui dependensi Python yang dibutuhkan...
    "%PY_CMD%" -m pip install -q requests yt-dlp
)

:: 4. Buka Browser otomatis setelah 2 detik
start "" cmd /c "ping 127.0.0.1 -n 3 >nul && start http://127.0.0.1:6899/"

echo [*] Menjalankan Server Core di http://127.0.0.1:6899 ...
echo [*] Browser akan terbuka secara otomatis.
echo.
echo =========================================================================
echo  JANGAN TUTUP JENDELA INI SELAMA MENGGUNAKAN OPENIDM
echo  Tekan Ctrl+C di sini untuk menghentikan server
echo =========================================================================
echo.

"%PY_CMD%" main.py

if errorlevel 1 (
    echo.
    echo [!] Aplikasi terhenti dengan kode error: %errorlevel%
    pause
)
