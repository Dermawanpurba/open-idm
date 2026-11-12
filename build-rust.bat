@echo off
setlocal
title OpenIDM - Build aplikasi Rust
chcp 65001 >nul
cd /d "%~dp0"

rem Toolchain Rust (GNU) dan MinGW-w64 berada di D:\rust. Ubah RUST_ROOT bila dipindah.
if not defined RUST_ROOT set "RUST_ROOT=D:\rust"
set "RUSTUP_HOME=%RUST_ROOT%\rustup"
set "CARGO_HOME=%RUST_ROOT%\cargo"
set "PATH=%RUST_ROOT%\cargo\bin;%RUST_ROOT%\mingw64\bin;%PATH%"
set "OUT=%RUST_ROOT%\target\openidm\release"

set "DEST=%~1"
if "%DEST%"=="" set "DEST=%~dp0..\open-idm"
for %%I in ("%DEST%") do set "DEST=%%~fI"

where cargo >nul 2>&1
if errorlevel 1 (
    echo [ERROR] cargo tidak ditemukan di %RUST_ROOT%\cargo\bin
    exit /b 1
)
for %%F in (yt-dlp.exe ffmpeg.exe node.exe) do (
    if not exist "bin\%%F" (
        echo [ERROR] Helper video bin\%%F belum tersedia. Lengkapi helper sebelum build paket.
        exit /b 1
    )
)

echo [1/4] Menjalankan test Rust...
cargo test --workspace
if errorlevel 1 (
    echo [ERROR] Test Rust gagal. Build dibatalkan.
    exit /b 1
)

echo [2/4] Menjalankan test extension...
node --test tests\extension.test.cjs tests\ui-api.test.cjs tests\ui-segments.test.cjs
if errorlevel 1 (
    echo [ERROR] Test extension gagal. Build dibatalkan.
    exit /b 1
)

echo [3/4] Build rilis...
cargo build --release -p openidm -p openidm-host --features tauri/custom-protocol
if errorlevel 1 (
    echo [ERROR] Build gagal.
    exit /b 1
)

echo [4/4] Menyalin ke %DEST% ...
set "OPENIDM_PACKAGE_TARGET=%DEST%\OpenIDM.exe"
powershell -NoProfile -Command "if (@(Get-Process OpenIDM -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $env:OPENIDM_PACKAGE_TARGET }).Count -gt 0) { exit 1 }"
if errorlevel 1 (
    echo [ERROR] Paket tujuan masih berjalan: %OPENIDM_PACKAGE_TARGET%
    echo Keluar dari paket tujuan lewat ikon tray, lalu jalankan lagi.
    exit /b 1
)
if not exist "%DEST%" mkdir "%DEST%"
copy /Y "%OUT%\OpenIDM.exe" "%DEST%\" >nul || exit /b 1
copy /Y "%OUT%\openidm-host.exe" "%DEST%\" >nul || exit /b 1
rem Target GNU memuat WebView2 lewat DLL ini; wajib berada di samping OpenIDM.exe.
copy /Y "%OUT%\WebView2Loader.dll" "%DEST%\" >nul || exit /b 1
robocopy "browser-extension" "%DEST%\browser-extension" /E /NFL /NDL /NJH /NJS /NP >nul
if errorlevel 8 (
    echo [ERROR] Gagal menyalin folder browser-extension.
    exit /b 1
)
copy /Y "packaging\Jalankan-OpenIDM.bat" "%DEST%\" >nul || exit /b 1
copy /Y "docs\portable-rust.txt" "%DEST%\CARA-PAKAI.txt" >nul || exit /b 1
copy /Y "packaging\LICENSE" "%DEST%\LICENSE" >nul || exit /b 1
if not exist "%DEST%\bin" mkdir "%DEST%\bin"
for %%F in (yt-dlp.exe ffmpeg.exe node.exe NODE-LICENSE.txt yt-dlp-release.json YT-DLP-LICENSE.txt) do (
    if exist "bin\%%F" copy /Y "bin\%%F" "%DEST%\bin\" >nul || exit /b 1
)

echo.
echo Selesai. OpenIDM.exe, openidm-host.exe, WebView2Loader.dll, dan browser-extension ada di %DEST%
exit /b 0
