@echo off
echo ==========================================
echo   AparatKids Telegram Bot - Full Install
echo ==========================================
echo.

echo [1/3] Installing Python packages...
pip install -r requirements.txt
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo ERROR: pip install failed. Make sure Python is installed.
    echo Download from: https://www.python.org/downloads/
    pause
    exit /b 1
)

echo.
echo [2/3] Configuring bot...
python -c "import json,os;cfg='config.json';d=json.load(open(cfg)) if os.path.exists(cfg) else {'bot_token':'','admin_id':0,'premium_users':[],'daily_limit':5,'max_file_size_mb':50};exec('' if d.get('bot_token') and d.get('admin_id') else 'print();t=input(\"Enter Bot Token: \").strip();i=int(input(\"Enter Admin Telegram ID: \").strip());d[\"bot_token\"]=t;d[\"admin_id\"]=i;json.dump(d,open(cfg,\"w\"),indent=2,ensure_ascii=False);print(\"Config saved!\")' if not d.get('bot_token') or not d.get('admin_id') else 'print(\"Config OK.\")')"

echo.
echo [3/3] Creating directories...
if not exist downloads mkdir downloads

echo.
echo ==========================================
echo   Installation complete!
echo   Run: python bot.py
echo   Or use: run.bat
echo ==========================================
pause
