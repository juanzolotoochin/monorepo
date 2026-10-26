@echo off
rem Runs network-lab.ps1 from cmd. Arguments pass through unchanged, e.g.
rem   kestrel\tools\network-lab.bat -DelayMs 100 -JitterMs 20 -Build
rem See network-lab.ps1 for the options.
setlocal
where pwsh >nul 2>nul
if %errorlevel%==0 (set "PS=pwsh") else (set "PS=powershell")
"%PS%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0network-lab.ps1" %*
exit /b %errorlevel%
