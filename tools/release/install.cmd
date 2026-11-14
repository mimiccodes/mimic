@echo off
rem Install mimic: double-click this file, or run `install.cmd` (`install.cmd -Uninstall` removes it).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
echo.
pause
