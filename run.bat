@echo off
setlocal enabledelayedexpansion

:: ============================================
::   VideoHunter - Setup ^& Launcher
:: ============================================

:: --- Check Prerequisites ---
call :check_node
call :check_rust
call :check_cargo_about
call :check_npm_deps

:menu
cls
echo ==============================
echo   VideoHunter
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
echo.
if exist "src-tauri\target\release\bundle\nsis\VideoHunter_*_x64-setup.exe" (
    echo Build successful! Installer created in src-tauri\target\release\bundle\nsis\
) else if exist "src-tauri\target\release\videohunter.exe" (
    echo Build successful! Executable at src-tauri\target\release\videohunter.exe
)
pause
goto menu

:end
exit

:: ============================================
::   Prerequisite Check Functions
:: ============================================

:check_node
where node >nul 2>nul
if %errorlevel% neq 0 (
    echo [!] Node.js not found. Installing...
    powershell -Command "& {[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri 'https://nodejs.org/dist/v24.4.0/node-v24.4.0-x64.msi' -OutFile '$env:TEMP\node-install.msi'; Start-Process msiexec.exe -ArgumentList '/i', \"$env:TEMP\node-install.msi\", '/qn', 'ADDLOCAL=ALL' -Wait}"
    if %errorlevel% neq 0 (
        echo [X] Node.js installation failed. Please install manually from https://nodejs.org
        pause
        exit /b 1
    )
    :: Refresh PATH
    set "PATH=%PATH%;C:\Program Files\nodejs\"
    echo [OK] Node.js installed.
) else (
    for /f "tokens=*" %%v in ('node --version 2^>nul') do set NODE_VER=%%v
    echo [OK] Node.js found: !NODE_VER!
)
exit /b 0

:check_rust
where rustc >nul 2>nul
if %errorlevel% neq 0 (
    echo [!] Rust not found. Installing via rustup...
    powershell -Command "& {[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri 'https://win.rustup.rs/x86_64' -OutFile '$env:TEMP\rustup-init.exe'; Start-Process -FilePath '$env:TEMP\rustup-init.exe' -ArgumentList '-y' -Wait}"
    if %errorlevel% neq 0 (
        echo [X] Rust installation failed. Please install manually from https://rustup.rs
        pause
        exit /b 1
    )
    :: Refresh PATH
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    echo [OK] Rust installed.
) else (
    for /f "tokens=2 delims= " %%v in ('rustc --version 2^>nul') do set RUST_VER=%%v
    echo [OK] Rust found: !RUST_VER!
)
exit /b 0

:check_cargo_about
where cargo-about >nul 2>nul
if %errorlevel% neq 0 (
    echo [!] cargo-about not found. Installing...
    cargo install cargo-about --features cli
    if %errorlevel% neq 0 (
        echo [X] cargo-about installation failed. Build may fail on license step.
        echo     Continuing anyway...
    ) else (
        echo [OK] cargo-about installed.
    )
) else (
    echo [OK] cargo-about found.
)
exit /b 0

:check_npm_deps
if not exist "node_modules" (
    echo [!] node_modules not found. Running npm install...
    call npm install
    if %errorlevel% neq 0 (
        echo [X] npm install failed.
        pause
        exit /b 1
    )
    echo [OK] Dependencies installed.
) else (
    echo [OK] node_modules found.
)
exit /b 0
