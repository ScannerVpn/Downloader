@echo off
:menu
cls
echo ==============================
echo   AparatKids Downloader
echo ==============================
echo  1. Run Dev Mode
echo  2. Build App
echo  3. Exit
echo ==============================
set /p choice="Select option: "

if "%choice%"=="1" goto dev
if "%choice%"=="2" goto build
if "%choice%"=="3" goto end

echo Invalid option!
pause
goto menu

:dev
cls
echo Starting dev mode...
npm run tauri dev
pause
goto menu

:build
cls
echo Building app...
npm run tauri build
pause
goto menu

:end
exit
