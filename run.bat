@echo off
title OpenIDM - Desktop Core
chcp 65001 >nul
color 0b
cd /d "%~dp0"

echo ===================================================
echo     Memulai OpenIDM Core & Dashboard Desktop...
echo ===================================================
echo.

python main.py

pause
