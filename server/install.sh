#!/bin/bash
set -e

echo "=========================================="
echo "  AparatKids Telegram Bot - Full Install"
echo "=========================================="

# Detect package manager
if command -v apt-get &> /dev/null; then
    PKG="apt-get"
elif command -v yum &> /dev/null; then
    PKG="yum"
elif command -v dnf &> /dev/null; then
    PKG="dnf"
elif command -v apk &> /dev/null; then
    PKG="apk"
elif command -v pacman &> /dev/null; then
    PKG="pacman"
else
    echo "Package manager not found. Install python3, pip, ffmpeg manually."
    exit 1
fi

echo "[1/5] Installing system dependencies..."
if [ "$PKG" = "apt-get" ]; then
    sudo apt-get update -qq
    sudo apt-get install -y -qq python3 python3-pip ffmpeg
elif [ "$PKG" = "yum" ]; then
    sudo yum install -y python3 python3-pip ffmpeg
elif [ "$PKG" = "dnf" ]; then
    sudo dnf install -y python3 python3-pip ffmpeg
elif [ "$PKG" = "apk" ]; then
    apk add python3 py3-pip ffmpeg
elif [ "$PKG" = "pacman" ]; then
    sudo pacman -S --noconfirm python python-pip ffmpeg
fi

echo "[2/5] Installing Python packages..."
pip3 install -r requirements.txt

echo "[3/5] Configuring bot..."
python3 -c "
import json, os
cfg_path = 'config.json'
if os.path.exists(cfg_path):
    with open(cfg_path) as f:
        cfg = json.load(f)
    if cfg.get('bot_token') and cfg.get('admin_id', 0) != 0:
        print('Config already exists. Skipping setup.')
        exit(0)

print()
print('--- Telegram Bot Setup ---')
print()
token = input('Enter Bot Token from @BotFather: ').strip()
while True:
    try:
        admin_id = int(input('Enter your Telegram User ID (numeric): ').strip())
        break
    except ValueError:
        print('Invalid! Enter a numeric ID.')

import json
cfg = {'bot_token': token, 'admin_id': admin_id, 'premium_users': [], 'daily_limit': 5, 'max_file_size_mb': 50}
with open(cfg_path, 'w') as f:
    json.dump(cfg, f, indent=2, ensure_ascii=False)
print('Config saved!')
"

echo "[4/5] Creating directories..."
mkdir -p downloads

echo "[5/5] Setting up systemd service..."
if [ "$PKG" = "apt-get" ] || [ "$PKG" = "yum" ] || [ "$PKG" = "dnf" ]; then
    BOT_DIR="$(cd "$(dirname "$0")" && pwd)"
    sudo tee /etc/systemd/system/aparatkids-bot.service > /dev/null << EOF
[Unit]
Description=AparatKids Telegram Bot
After=network.target

[Service]
Type=simple
WorkingDirectory=$BOT_DIR
ExecStart=$(which python3) bot.py
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
    sudo systemctl daemon-reload
    sudo systemctl enable aparatkids-bot
    sudo systemctl start aparatkids-bot
    echo "Service installed and started!"
else
    echo "Start manually: python3 bot.py"
fi

echo ""
echo "=========================================="
echo "  Installation complete!"
echo "  Bot is running in background."
echo "  Service: systemctl status aparatkids-bot"
echo "  Logs:    journalctl -u aparatkids-bot -f"
echo "=========================================="
