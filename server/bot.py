#!/usr/bin/env python3
"""
AparatKids Telegram Downloader Bot
Server-side bot for downloading videos via Telegram.
"""

import asyncio
import json
import os
import re
import subprocess
import sys
import time
from datetime import datetime, timedelta
from pathlib import Path

from telegram import (
    BotCommand,
    InlineKeyboardButton,
    InlineKeyboardMarkup,
    Update,
)
from telegram.ext import (
    Application,
    CallbackQueryHandler,
    CommandHandler,
    ContextTypes,
    MessageHandler,
    filters,
)

# --- Config ---
CONFIG_PATH = Path(__file__).parent / "config.json"
DOWNLOAD_DIR = Path(__file__).parent / "downloads"
USAGE_FILE = Path(__file__).parent / "usage.json"

DOWNLOAD_DIR.mkdir(exist_ok=True)


def load_config():
    with open(CONFIG_PATH, "r", encoding="utf-8") as f:
        return json.load(f)


def save_config(cfg):
    with open(CONFIG_PATH, "w", encoding="utf-8") as f:
        json.dump(cfg, f, indent=2, ensure_ascii=False)


def load_usage():
    if USAGE_FILE.exists():
        with open(USAGE_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    return {}


def save_usage(data):
    with open(USAGE_FILE, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)


def get_today_key():
    return datetime.now().strftime("%Y-%m-%d")


def get_user_usage(user_id: int) -> int:
    usage = load_usage()
    today = get_today_key()
    return usage.get(str(user_id), {}).get(today, 0)


def increment_user_usage(user_id: int):
    usage = load_usage()
    uid = str(user_id)
    today = get_today_key()
    if uid not in usage:
        usage[uid] = {}
    usage[uid][today] = usage[uid].get(today, 0) + 1
    save_usage(usage)


def can_download(user_id: int) -> bool:
    cfg = load_config()
    if user_id == cfg["admin_id"]:
        return True
    if user_id in cfg["premium_users"]:
        return True
    return get_user_usage(user_id) < cfg["daily_limit"]


def get_remaining(user_id: int) -> int:
    cfg = load_config()
    if user_id == cfg["admin_id"] or user_id in cfg["premium_users"]:
        return -1
    return max(0, cfg["daily_limit"] - get_user_usage(user_id))


def is_aparat_url(url: str) -> bool:
    patterns = [
        r"https?://(?:www\.)?aparat\.com/(?:v|m|shorts)/",
        r"https?://(?:www\.)?aparatkids\.com/(?:w|m)/",
    ]
    return any(re.search(p, url) for p in patterns)


# --- Setup ---
def first_time_setup():
    cfg = load_config()
    if cfg["bot_token"] and cfg["admin_id"] != 0:
        return cfg

    print("=" * 50)
    print("  AparatKids Telegram Downloader - Setup")
    print("=" * 50)
    print()

    if not cfg["bot_token"]:
        cfg["bot_token"] = input("Enter your Telegram Bot Token: ").strip()

    if cfg["admin_id"] == 0:
        while True:
            try:
                cfg["admin_id"] = int(input("Enter your Telegram User ID (numeric): ").strip())
                break
            except ValueError:
                print("Invalid! Enter a numeric Telegram User ID.")

    save_config(cfg)
    print("\nSetup complete! Config saved.\n")
    return cfg


# --- URL validation ---
def extract_url(text: str) -> str | None:
    url_pattern = re.compile(r"https?://[^\s<>\"']+")
    match = url_pattern.search(text)
    return match.group(0) if match else None


# --- Download ---
async def download_video(url: str, output_dir: Path) -> dict:
    """Download video using yt-dlp. Returns dict with file_path, title, thumbnail, duration."""
    output_template = str(output_dir / "%(id)s.%(ext)s")

    cmd = [
        "yt-dlp",
        "--encoding", "utf-8",
        "-f", "bv*+ba/b",
        "-o", output_template,
        "--no-playlist",
        "--print-json",
        "--no-warnings",
        "--no-colors",
        "--restrict-filenames",
        "--merge-output-format", "mp4",
        url,
    ]

    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()

    if proc.returncode != 0:
        error_msg = stderr.decode("utf-8", errors="replace").strip()
        return {"error": error_msg or "Download failed"}

    try:
        info = json.loads(stdout.decode("utf-8", errors="replace"))
    except json.JSONDecodeError:
        return {"error": "Failed to parse yt-dlp output"}

    video_id = info.get("id", "unknown")
    ext = info.get("ext", "mp4")
    file_path = output_dir / f"{video_id}.{ext}"

    if not file_path.exists():
        for f in output_dir.glob(f"{video_id}.*"):
            file_path = f
            break

    return {
        "file_path": str(file_path),
        "title": info.get("title", ""),
        "thumbnail": info.get("thumbnail", ""),
        "duration": info.get("duration", 0),
        "filesize": info.get("filesize") or info.get("filesize_approx"),
    }


def format_duration(seconds):
    if not seconds:
        return ""
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = int(seconds % 60)
    if h > 0:
        return f"{h}:{m:02d}:{s:02d}"
    return f"{m}:{s:02d}"


def format_size(size_bytes):
    if not size_bytes:
        return "نامشخص"
    for unit in ["B", "KB", "MB", "GB"]:
        if size_bytes < 1024:
            return f"{size_bytes:.1f} {unit}"
        size_bytes /= 1024
    return f"{size_bytes:.1f} TB"


# --- Handlers ---
async def cmd_start(update: Update, context: ContextTypes.DEFAULT_TYPE):
    user = update.effective_user
    cfg = load_config()

    keyboard = [
        [InlineKeyboardButton("📥 دانلود ویدیو", callback_data="menu_download")],
        [InlineKeyboardButton("👤 پروفایل من", callback_data="menu_profile")],
    ]

    if user.id == cfg["admin_id"]:
        keyboard.append([InlineKeyboardButton("⚙️ پنل مدیریت", callback_data="menu_admin")])

    reply_markup = InlineKeyboardMarkup(keyboard)

    text = (
        f"سلام {user.first_name}! 👋\n\n"
        "🎬 ربات دانلود ویدیو آپارات کودک\n\n"
        "لینک ویدیو رو بفرست تا دانلود کنم!"
    )

    if update.callback_query:
        await update.callback_query.edit_message_text(text, reply_markup=reply_markup)
    else:
        await update.message.reply_text(text, reply_markup=reply_markup)


async def cmd_help(update: Update, context: ContextTypes.DEFAULT_TYPE):
    text = (
        "📖 راهنما:\n\n"
        "1️⃣ لینک ویدیوی آپارات یا آپارات کودک رو بفرست\n"
        "2️⃣ ربات ویدیو رو دانلود و ارسال می‌کنه\n\n"
        "🌐 سایت‌های پشتیبانی شده:\n"
        "• aparat.com\n"
        "• aparatkids.com\n\n"
        "📋 محدودیت‌ها:\n"
        "• کاربر عادی: ۵ دانلود در روز\n"
        "• کاربر ویژه: نامحدود\n"
        "• مدیر: نامحدود"
    )
    await update.message.reply_text(text)


async def cb_handler(update: Update, context: ContextTypes.DEFAULT_TYPE):
    query = update.callback_query
    await query.answer()
    data = query.data
    user_id = query.from_user.id

    if data == "menu_download":
        await query.edit_message_text(
            "📥 لینک ویدیو رو بفرست:\n\n"
            "مثال:\nhttps://www.aparat.com/v/abc123",
        )

    elif data == "menu_profile":
        cfg = load_config()
        remaining = get_remaining(user_id)
        role = "مدیر" if user_id == cfg["admin_id"] else ("ویژه" if user_id in cfg["premium_users"] else "عادی")
        limit_text = "نامحدود" if remaining == -1 else str(remaining)

        text = (
            f"👤 پروفایل شما:\n\n"
            f"身份: {query.from_user.first_name}\n"
            f"شناسه: {user_id}\n"
            f"نقش: {role}\n"
            f"دانلود باقی‌مانده امروز: {limit_text}\n"
            f"دانلود امروز: {get_user_usage(user_id)}"
        )
        keyboard = [[InlineKeyboardButton("🔙 بازگشت", callback_data="menu_back")]]
        await query.edit_message_text(text, reply_markup=InlineKeyboardMarkup(keyboard))

    elif data == "menu_back":
        await cmd_start(update, context)

    elif data == "menu_admin":
        await show_admin_panel(query)

    elif data.startswith("admin_add_premium_"):
        target_id = int(data.split("_")[-1])
        cfg = load_config()
        if target_id not in cfg["premium_users"]:
            cfg["premium_users"].append(target_id)
            save_config(cfg)
        await query.answer("✅ کاربر ویژه شد!")

    elif data.startswith("admin_remove_premium_"):
        target_id = int(data.split("_")[-1])
        cfg = load_config()
        if target_id in cfg["premium_users"]:
            cfg["premium_users"].remove(target_id)
            save_config(cfg)
        await query.answer("✅ از ویژه حذف شد!")

    elif data == "admin_list_users":
        await show_user_list(query)

    elif data == "admin_back":
        await show_admin_panel(query)

    elif data.startswith("dl_"):
        await handle_download_callback(query, data, context)


async def show_admin_panel(query):
    cfg = load_config()
    usage = load_usage()
    today = get_today_key()

    total_users = len(usage)
    active_today = sum(1 for u in usage.values() if today in u)
    premium_count = len(cfg["premium_users"])

    text = (
        "⚙️ پنل مدیریت\n\n"
        f"👥 کل کاربران: {total_users}\n"
        f"📊 فعال امروز: {active_today}\n"
        f"⭐ کاربران ویژه: {premium_count}\n"
        f"📋 محدودیت روزانه: {cfg['daily_limit']}\n"
    )

    keyboard = [
        [InlineKeyboardButton("👥 لیست کاربران", callback_data="admin_list_users")],
        [InlineKeyboardButton("🔙 بازگشت", callback_data="menu_back")],
    ]
    await query.edit_message_text(text, reply_markup=InlineKeyboardMarkup(keyboard))


async def show_user_list(query):
    usage = load_usage()
    cfg = load_config()
    today = get_today_key()

    text = "👥 لیست کاربران:\n\n"
    for uid, data in usage.items():
        count = data.get(today, 0)
        role = "⭐" if int(uid) in cfg["premium_users"] else ("👑" if int(uid) == cfg["admin_id"] else "👤")
        text += f"{role} ID: {uid} — امروز: {count}\n"

    keyboard = [[InlineKeyboardButton("🔙 بازگشت", callback_data="admin_back")]]
    await query.edit_message_text(text, reply_markup=InlineKeyboardMarkup(keyboard))


async def handle_download_callback(query, data, context):
    _, video_id, url = data.split("|", 2)
    await do_download(query.from_user.id, url, query.message, context)


async def handle_message(update: Update, context: ContextTypes.DEFAULT_TYPE):
    text = update.message.text
    url = extract_url(text)

    if not url:
        await update.message.reply_text("❌ لطفاً یک لینک معتبر بفرستید.")
        return

    if not is_aparat_url(url):
        await update.message.reply_text(
            "❌ این لینک پشتیبانی نمی‌شود.\n\n"
            "فقط لینک‌های aparat.com و aparatkids.com مجاز هستند."
        )
        return

    await do_download(update.effective_user.id, url, update.message, context)


async def do_download(user_id: int, url: str, message, context):
    if not can_download(user_id):
        await message.reply_text(
            "⛔ سقف دانلود روزانه شما تمام شده.\n"
            "برای افزایش سقف با مدیر تماس بگیرید."
        )
        return

    cfg = load_config()

    status_msg = await message.reply_text("⏳ در حال دانلود ویدیو...")

    try:
        result = await asyncio.wait_for(
            download_video(url, DOWNLOAD_DIR),
            timeout=120,
        )
    except asyncio.TimeoutError:
        await status_msg.edit_text("⏰ دانلود بیش از حد طول کشید. دوباره تلاش کنید.")
        return

    if "error" in result:
        await status_msg.edit_text(f"❌ خطا در دانلود:\n{result['error'][:500]}")
        return

    file_path = result["file_path"]
    title = result["title"] or "ویدیو"
    duration = format_duration(result.get("duration"))
    size = format_size(result.get("filesize"))

    if not os.path.exists(file_path):
        await status_msg.edit_text("❌ فایل ویدیو یافت نشد.")
        return

    file_size_mb = os.path.getsize(file_path) / (1024 * 1024)
    if file_size_mb > cfg["max_file_size_mb"]:
        await status_msg.edit_text(
            f"❌ حجم فایل ({file_size_mb:.1f} MB) بیشتر از حد مجاز ({cfg['max_file_size_mb']} MB) است."
        )
        os.remove(file_path)
        return

    caption = f"🎬 {title}"
    if duration:
        caption += f"\n⏱ {duration}"
    caption += f"\n📦 {size}"

    try:
        with open(file_path, "rb") as f:
            await message.reply_video(
                video=f,
                caption=caption,
                read_timeout=300,
                write_timeout=300,
            )
        await status_msg.delete()
        increment_user_usage(user_id)
    except Exception as e:
        await status_msg.edit_text(f"❌ خطا در ارسال فایل:\n{str(e)[:300]}")
    finally:
        if os.path.exists(file_path):
            os.remove(file_path)


async def post_init(application: Application):
    await application.bot.set_my_commands([
        BotCommand("start", "شروع / منوی اصلی"),
        BotCommand("help", "راهنما"),
    ])


def main():
    cfg = first_time_setup()

    print("Starting AparatKids Telegram Bot...")

    app = Application.builder().token(cfg["bot_token"]).post_init(post_init).build()

    app.add_handler(CommandHandler("start", cmd_start))
    app.add_handler(CommandHandler("help", cmd_help))
    app.add_handler(CallbackQueryHandler(cb_handler))
    app.add_handler(MessageHandler(filters.TEXT & ~filters.COMMAND, handle_message))

    print("Bot is running!")
    app.run_polling(allowed_updates=Update.ALL_TYPES)


if __name__ == "__main__":
    main()
