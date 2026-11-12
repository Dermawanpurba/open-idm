@echo off
setlocal
set "OPENIDM_APP=%~dp0..\open-idm\OpenIDM.exe"
if not exist "%OPENIDM_APP%" (
    echo Paket Rust belum tersedia. Jalankan build-rust.bat terlebih dahulu.
    exit /b 1
)
start "" "%OPENIDM_APP%"
exit /b
