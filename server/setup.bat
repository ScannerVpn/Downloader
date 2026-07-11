@echo off
echo ==============================
echo   AparatKids Bot - Setup
echo ==============================
echo.
echo Installing dependencies...
pip install -r requirements.txt
echo.
echo Running bot (will ask for config on first run)...
python bot.py
pause
