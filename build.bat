@echo off
call "%~dp0build-rust.bat" %*
exit /b %errorlevel%
