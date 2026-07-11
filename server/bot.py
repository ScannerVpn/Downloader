#!/usr/bin/env python3
"""
AparatKids Telegram Downloader Bot
Server-side bot for downloading videos via Telegram.
"""

import asyncio
import json
import os
import re
import sys
import tempfile
from datetime import datetime
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
VENV_DIR = Path(__file__).parent / "venv"

# Resolve yt-dlp path: prefer venv, fall back to system
import shutil
_ytdlp_candidates = [
    VENV_DIR / "bin" / "yt-dlp",
    VENV_DIR / "Scripts" / "yt-dlp.exe",
]
YTDLP = next((str(p) for p in _ytdlp_candidates if p.exists()), None)
if not YTDLP:
    YTDLP = shutil.which("yt-dlp") or "yt-dlp"

DOWNLOAD_DIR.mkdir(exist_ok=True)

# YouTube links that are allowed (in addition to aparat)
ALLOWED_DOMAINS = [
    "aparat.com", "aparatkids.com",
    "youtube.com", "youtu.be",
    "instagram.com", "www.instagram.com",
    "tiktok.com", "vm.tiktok.com",
    "twitter.com", "x.com",
    "facebook.com", "fb.watch",
    "reddit.com", "v.redd.it",
    "dailymotion.com",
    "twitch.tv",
    "soundcloud.com",
]


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


def is_allowed_url(url: str) -> bool:
    from urllib.parse import urlparse
    try:
        parsed = urlparse(url)
        host = parsed.hostname or ""
        return any(host == d or host.endswith("." + d) for d in ALLOWED_DOMAINS)
    except Exception:
        return False


def extract_url(text: str) -> str | None:
    url_pattern = re.compile(r"https?://[^\s<>\"']+")
    match = url_pattern.search(text)
    return match.group(0) if match else None


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


# --- yt-dlp helpers ---
async def yt_dlp_info(url: str) -> dict | None:
    """Get video/playlist info from yt-dlp."""
    cmd = [
        YTDLP,
        "--encoding", "utf-8",
        "--dump-json",
        "--flat-playlist",
        "--no-warnings",
        "--no-colors",
        url,
    ]
    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()
    if proc.returncode != 0:
        return None
    try:
        return json.loads(stdout.decode("utf-8", errors="replace"))
    except json.JSONDecodeError:
        return None


async def fetch_aparat_playlist(url: str) -> list:
    """Fetch aparat playlist entries via API (detect ?playlist= param)."""
    import re as _re
    match = _re.search(r"[?&]playlist=(\d+)", url)
    if not match:
        return []

    # Extract video hash from URL
    vid_match = _re.search(r"aparat\.com/v/([a-zA-Z0-9]+)", url)
    if not vid_match:
        return []
    video_hash = vid_match.group(1)

    api_url = f"https://www.aparat.com/api/fa/v1/video/video/show/videohash/{video_hash}?pr=1&mf=1"

    cmd = [
        "curl", "-s", "-H", "Referer: https://www.aparat.com/",
        "-H", "Accept: application/json", api_url,
    ]
    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, _ = await proc.communicate()

    try:
        data = json.loads(stdout.decode("utf-8", errors="replace"))
    except (json.JSONDecodeError, UnicodeDecodeError):
        return []

    # Find playlist in included and extract video IDs
    included = data.get("included", [])
    playlist = None
    for item in included:
        if item.get("type") == "playlist":
            playlist = item
            break

    if not playlist:
        return []

    video_ids = [
        v["id"] for v in playlist.get("relationships", {}).get("video", {}).get("data", [])
    ]

    # Map video IDs to UIDs and titles
    id_to_uid = {}
    id_to_title = {}
    for item in included:
        if item.get("type") == "Video":
            vid_id = item.get("id", "")
            attrs = item.get("attributes", {})
            uid = attrs.get("uid", "")
            title = attrs.get("title", "")
            if vid_id and uid:
                id_to_uid[vid_id] = uid
                id_to_title[vid_id] = title

    entries = []
    for vid_id in video_ids:
        uid = id_to_uid.get(vid_id)
        if uid:
            entries.append({
                "url": f"https://www.aparat.com/v/{uid}",
                "title": id_to_title.get(vid_id, ""),
            })

    return entries


async def yt_dlp_download(url: str, output_path: str, format_spec: str = "bv*+ba/b") -> dict:
    """Download a single video using yt-dlp."""
    cmd = [
        YTDLP,
        "--encoding", "utf-8",
        "-f", format_spec,
        "-o", output_path,
        "--no-playlist",
        "--merge-output-format", "mp4",
        "--restrict-filenames",
        "--no-warnings",
        "--no-colors",
        "--print-json",
        url,
    ]
    proc = await asyncio.create_subprocess_exec(
        *cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()

    if proc.returncode != 0:
        return {"error": stderr.decode("utf-8", errors="replace").strip()[:500]}

    try:
        info = json.loads(stdout.decode("utf-8", errors="replace"))
    except json.JSONDecodeError:
        return {"error": "Failed to parse yt-dlp output"}

    # Find the actual downloaded file
    video_id = info.get("id", "unknown")
    ext = info.get("ext", "mp4")
    file_path = Path(output_path).parent / f"{video_id}.{ext}"

    # Try to find the file with any extension
    if not file_path.exists():
        parent = Path(output_path).parent
        for f in parent.glob(f"{video_id}.*"):
            file_path = f
            break

    return {
        "file_path": str(file_path),
        "title": info.get("title", ""),
        "thumbnail": info.get("thumbnail", ""),
        "duration": info.get("duration", 0),
        "filesize": info.get("filesize") or info.get("filesize_approx"),
        "formats": info.get("formats", []),
    }



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


# --- Handlers ---
async def cmd_start(update: Update, context: ContextTypes.DEFAULT_TYPE):
    user = update.effective_user
    cfg = load_config()

    keyboard = [
        [InlineKeyboardButton("📥 دانلود ویدیو", callback_data="menu_download")],
        [InlineKeyboardButton("👤 پروفایل من", callback_data="menu_profile")],
        [InlineKeyboardButton("📋 پشتیبانی از سایت‌ها", callback_data="menu_sites")],
    ]

    if user.id == cfg["admin_id"]:
        keyboard.append([InlineKeyboardButton("⚙️ پنل مدیریت", callback_data="menu_admin")])

    reply_markup = InlineKeyboardMarkup(keyboard)

    text = (
        f"سلام {user.first_name}! 👋\n\n"
        "🎬 ربات دانلود ویدیو\n\n"
        "لینک ویدیو رو بفرست تا دانلود کنم!\n\n"
        "🌐 سایت‌های پشتیبانی شده:\n"
        "آپارات • یوتیوپ • اینستاگرام • تیک‌تاک\n"
        "توییتر • فیسبوک • ردیت • توییچ • ساندکلود"
    )

    if update.callback_query:
        await update.callback_query.edit_message_text(text, reply_markup=reply_markup)
    else:
        await update.message.reply_text(text, reply_markup=reply_markup)


async def cmd_help(update: Update, context: ContextTypes.DEFAULT_TYPE):
    text = (
        "📖 راهنما:\n\n"
        "1️⃣ لینک ویدیو بفرست\n"
        "2️⃣ کیفیت دلخواه رو انتخاب کن\n"
        "3️⃣ ویدیو دانلود و ارسال میشه\n\n"
        "📋 پلی‌لیست:\n"
        "• لینک پلی‌لیست بفرست\n"
        "• لیست ویدیوها نمایش داده میشه\n"
        "• دانلود تکی یا دانلود همه\n\n"
        "🌐 سایت‌های پشتیبانی شده:\n"
        "آپارات • آپارات کودک • یوتیوپ\n"
        "اینستاگرام • تیک‌تاک • توییتر\n"
        "فیسبوک • ردیت • توییچ • ساندکلود\n\n"
        "📋 محدودیت‌ها:\n"
        "• کاربر عادی: ۵ دانلود در روز\n"
        "• کاربر ویژه: نامحدود\n"
        "• مدیر: نامحدود"
    )
    await update.message.reply_text(text)


async def cmd_update(update: Update, context: ContextTypes.DEFAULT_TYPE):
    cfg = load_config()
    if update.effective_user.id != cfg["admin_id"]:
        await update.message.reply_text("⛔ فقط مدیر می‌تونه آپدیت کنه.")
        return

    status = await update.message.reply_text("🔄 در حال آپدیت...")

    proc = await asyncio.create_subprocess_shell(
        "cd /root/Downloader && git pull origin main",
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await proc.communicate()
    output = stdout.decode("utf-8", errors="replace").strip()

    if "Already up to date" in output:
        await status.edit_text("✅ کد آپدیت هست. نیازی به ریستارت نیست.")
    elif proc.returncode == 0:
        await status.edit_text(
            f"✅ آپدیت شد!\n\n{output}\n\n"
            "🔄 ریستارت سرویس..."
        )
        proc2 = await asyncio.create_subprocess_shell(
            "systemctl restart aparatkids-bot",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        await proc2.communicate()
        await status.edit_text("✅ آپدیت و ریستارت انجام شد!")
    else:
        await status.edit_text(f"❌ خطا در آپدیت:\n{stderr.decode('utf-8', errors='replace')[:500]}")


async def cb_handler(update: Update, context: ContextTypes.DEFAULT_TYPE):
    query = update.callback_query
    await query.answer()
    data = query.data
    user_id = query.from_user.id

    if data == "menu_download":
        await query.edit_message_text(
            "📥 لینک ویدیو رو بفرست:\n\n"
            "مثال:\n"
            "https://www.aparat.com/v/abc123\n"
            "https://www.youtube.com/watch?v=xyz\n"
            "https://www.instagram.com/reel/xyz"
        )

    elif data == "menu_profile":
        cfg = load_config()
        remaining = get_remaining(user_id)
        role = "مدیر" if user_id == cfg["admin_id"] else ("ویژه" if user_id in cfg["premium_users"] else "عادی")
        limit_text = "نامحدود" if remaining == -1 else str(remaining)

        text = (
            f"👤 پروفایل شما:\n\n"
            f"نام: {query.from_user.first_name}\n"
            f"شناسه: {user_id}\n"
            f"نقش: {role}\n"
            f"دانلود باقی‌مانده امروز: {limit_text}\n"
            f"دانلود امروز: {get_user_usage(user_id)}"
        )
        keyboard = [[InlineKeyboardButton("🔙 بازگشت", callback_data="menu_back")]]
        await query.edit_message_text(text, reply_markup=InlineKeyboardMarkup(keyboard))

    elif data == "menu_sites":
        text = (
            "🌐 سایت‌های پشتیبانی شده:\n\n"
            "✅ آپارات (aparat.com)\n"
            "✅ آپارات کودک (aparatkids.com)\n"
            "✅ یوتیوپ (youtube.com / youtu.be)\n"
            "✅ اینستاگرام (instagram.com)\n"
            "✅ تیک‌تاک (tiktok.com)\n"
            "✅ توییتر / X (twitter.com / x.com)\n"
            "✅ فیسبوک (facebook.com / fb.watch)\n"
            "✅ ردیت (reddit.com / v.redd.it)\n"
            "✅ دیلی‌موشن (dailymotion.com)\n"
            "✅ توییچ (twitch.tv)\n"
            "✅ ساندکلود (soundcloud.com)"
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
        await show_admin_panel(query)

    elif data.startswith("admin_remove_premium_"):
        target_id = int(data.split("_")[-1])
        cfg = load_config()
        if target_id in cfg["premium_users"]:
            cfg["premium_users"].remove(target_id)
            save_config(cfg)
        await query.answer("✅ از ویژه حذف شد!")
        await show_admin_panel(query)

    elif data == "admin_list_users":
        await show_user_list(query)

    elif data == "admin_back":
        await show_admin_panel(query)

    elif data == "cancel_dl":
        context.user_data["cancel_download"] = True
        await query.answer("🚫 در حال لغو...")

    elif data.startswith("dl_one_"):
        # Download single video from playlist by index
        idx = int(data.split("_")[-1])
        entries = context.user_data.get("playlist_entries", [])
        context.user_data["pending_playlist_idx"] = idx
        context.user_data["pending_action"] = "single"
        if idx < len(entries):
            await show_quality_selection(query, idx, context)

    elif data == "dl_all":
        # Download all from playlist
        entries = context.user_data.get("playlist_entries", [])
        context.user_data["pending_action"] = "all"
        await show_quality_selection(query, -1, context)

    elif data.startswith("ql_"):
        # Quality selected - store format and trigger download
        fmt_key = data[3:]  # Remove "ql_" prefix
        action = context.user_data.get("pending_action", "all")
        entries = context.user_data.get("playlist_entries", [])
        idx = context.user_data.get("pending_playlist_idx", 0)

        # Resolve format spec from key
        fmt_map = {
            "best": "bv*+ba/b",
            "1080": "bv*[height<=1080]+ba/b",
            "720": "bv*[height<=720]+ba/b",
            "480": "bv*[height<=480]+ba/b",
            "360": "bv*[height<=360]+ba/b",
            "audio": "ba/b",
        }
        format_id = fmt_map.get(fmt_key, "bv*+ba/b")

        if action == "single" and idx < len(entries):
            entry = entries[idx]
            url = entry.get("url", "")
            await query.edit_message_text("⏳ در حال دانلود...")
            await do_download(query.from_user.id, url, format_id, query.message, context)
        elif action == "all":
            await query.edit_message_text(f"⏳ در حال دانلود {len(entries)} ویدیو...")
            await download_playlist(query, entries, format_id, context)


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
        [InlineKeyboardButton("🔄 آپدیت ربات", callback_data="admin_update")],
        [InlineKeyboardButton("🔙 بازگشت", callback_data="menu_back")],
    ]
    await query.edit_message_text(text, reply_markup=InlineKeyboardMarkup(keyboard))


async def show_user_list(query):
    usage = load_usage()
    cfg = load_config()
    today = get_today_key()

    text = "👥 لیست کاربران:\n\n"
    for uid, data in sorted(usage.items(), key=lambda x: x[1].get(today, 0), reverse=True):
        count = data.get(today, 0)
        if int(uid) == cfg["admin_id"]:
            role = "👑"
        elif int(uid) in cfg["premium_users"]:
            role = "⭐"
        else:
            role = "👤"
        text += f"{role} `{uid}` — امروز: {count}\n"

    if not usage:
        text += "هنوز کاربری ثبت نشده.\n"

    keyboard = [[InlineKeyboardButton("🔙 بازگشت", callback_data="admin_back")]]
    await query.edit_message_text(text, reply_markup=InlineKeyboardMarkup(keyboard), parse_mode="Markdown")


async def show_quality_selection(target, idx, context):
    """Show quality selection for a download."""
    entries = context.user_data.get("playlist_entries", [])

    if idx >= 0 and idx < len(entries):
        entry = entries[idx]
        title = entry.get("title", f"ویدیو {idx + 1}")
    else:
        title = f"دانلود همه ({len(entries)} ویدیو)"

    # Update the status message
    if hasattr(target, "edit_message_text"):
        await target.edit_message_text("⏳ دریافت کیفیت‌های موجود...")
    elif hasattr(target, "edit_text"):
        await target.edit_text("⏳ دریافت کیفیت‌های موجود...")

    keyboard = [
        [InlineKeyboardButton("📹 بهترین کیفیت", callback_data="ql_best")],
        [InlineKeyboardButton("📹 1080p", callback_data="ql_1080")],
        [InlineKeyboardButton("📹 720p", callback_data="ql_720")],
        [InlineKeyboardButton("📹 480p", callback_data="ql_480")],
        [InlineKeyboardButton("📹 360p", callback_data="ql_360")],
        [InlineKeyboardButton("🎵 فقط صدا", callback_data="ql_audio")],
    ]

    text = f"🎬 {title}\n\nکیفیت مورد نظر رو انتخاب کن:"
    if hasattr(target, "edit_message_text"):
        await target.edit_message_text(text, reply_markup=InlineKeyboardMarkup(keyboard))
    elif hasattr(target, "edit_text"):
        await target.edit_text(text, reply_markup=InlineKeyboardMarkup(keyboard))


async def handle_message(update: Update, context: ContextTypes.DEFAULT_TYPE):
    text = update.message.text
    url = extract_url(text)

    if not url:
        await update.message.reply_text("❌ لطفاً یک لینک معتبر بفرستید.")
        return

    if not is_allowed_url(url):
        await update.message.reply_text(
            "❌ این لینک پشتیبانی نمی‌شود.\n\n"
            "لیست سایت‌های مجاز:\n"
            "آپارات • یوتیوپ • اینستاگرام\n"
            "تیک‌تاک • توییتر • فیسبوک • ردیت"
        )
        return

    if not can_download(update.effective_user.id):
        await update.message.reply_text(
            "⛔ سقف دانلود روزانه شما تمام شده.\n"
            "برای افزایش سقف با مدیر تماس بگیرید."
        )
        return

    # Check if it might be a playlist
    status_msg = await update.message.reply_text("🔍 در حال بررسی لینک...")

    # First check for aparat playlist URLs
    aparat_entries = await fetch_aparat_playlist(url)
    if aparat_entries:
        info = {"_type": "playlist", "entries": aparat_entries, "url": url, "title": "آپارات پلی‌لیست"}
    else:
        info = await yt_dlp_info(url)

    if info and info.get("_type") == "playlist":
        # It's a playlist - show the list
        entries = info.get("entries", [])
        context.user_data["playlist_entries"] = entries
        context.user_data["pending_action"] = "all"

        playlist_title = info.get("title", "پلی‌لیست")
        text = f"📋 {playlist_title}\n\n"
        text += f"تعداد ویدیوها: {len(entries)}\n\n"
        text += "ویدیوها:\n"
        for i, entry in enumerate(entries[:20]):  # Show max 20
            title = entry.get("title", f"ویدیو {i+1}")
            text += f"  {i+1}. {title}\n"
        if len(entries) > 20:
            text += f"  ... و {len(entries) - 20} ویدیوی دیگر\n"

        keyboard = [
            [InlineKeyboardButton(f"📥 دانلود همه ({len(entries)})", callback_data="dl_all")],
        ]
        for i, entry in enumerate(entries[:10]):  # Max 10 individual buttons
            title = entry.get("title", f"ویدیو {i+1}")[:30]
            keyboard.append([InlineKeyboardButton(
                f"📹 {i+1}. {title}",
                callback_data=f"dl_one_{i}"
            )])

        await status_msg.edit_text(
            text,
            reply_markup=InlineKeyboardMarkup(keyboard),
        )
    else:
        # Single video - show quality selection
        context.user_data["playlist_entries"] = [{"url": url}]
        context.user_data["pending_action"] = "single"
        context.user_data["pending_playlist_idx"] = 0
        await show_quality_selection(status_msg, 0, context)


async def do_download(user_id: int, url: str, format_id: str, message, context):
    if not can_download(user_id):
        await message.reply_text("⛔ سقف دانلود روزانه شما تمام شده.")
        return

    cfg = load_config()

    with tempfile.TemporaryDirectory() as tmpdir:
        status_msg = await message.reply_text("⏳ در حال دانلود ویدیو...")

        result = await yt_dlp_download(
            url,
            os.path.join(tmpdir, "%(id)s.%(ext)s"),
            format_spec=format_id,
        )

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
            return

        caption = f"🎬 {title}"
        if duration and duration != "0:00":
            caption += f"\n⏱ {duration}"
        caption += f"\n📦 {size}"

        try:
            with open(file_path, "rb") as f:
                if file_size_mb <= 50:
                    await message.reply_video(
                        video=f,
                        caption=caption,
                        read_timeout=300,
                        write_timeout=300,
                    )
                else:
                    await message.reply_document(
                        document=f,
                        caption=caption,
                        read_timeout=300,
                        write_timeout=300,
                    )
            await status_msg.delete()
            increment_user_usage(user_id)
        except Exception as e:
            await status_msg.edit_text(f"❌ خطا در ارسال فایل:\n{str(e)[:300]}")


async def download_playlist(query, entries, format_id, context):
    """Download all videos in a playlist one by one."""
    user_id = query.from_user.id
    cfg = load_config()
    context.user_data["cancel_download"] = False

    total = len(entries)
    success = 0
    failed = 0

    cancel_keyboard = InlineKeyboardMarkup([
        [InlineKeyboardButton("❌ لغو دانلود", callback_data="cancel_dl")]
    ])

    for i, entry in enumerate(entries):
        # Check cancel
        if context.user_data.get("cancel_download"):
            await query.edit_message_text(
                f"🚫 دانلود لغو شد!\n\n"
                f"✅ موفق: {success}/{total}\n"
                f"❌ ناموفق: {failed}/{total}"
            )
            return

        url = entry.get("url") or entry.get("webpage_url", "")
        title = entry.get("title") or f"ویدیو {i + 1}"

        if not url:
            failed += 1
            continue

        if not can_download(user_id):
            await query.edit_message_text(
                f"⛔ سقف دانلود تمام شد!\n"
                f"✅ موفق: {success}/{total}\n"
                f"❌ ناموفق: {failed}/{total}"
            )
            return

        try:
            progress_pct = int((i / total) * 100)
            progress_bar = "█" * (progress_pct // 5) + "░" * (20 - progress_pct // 5)
            await query.edit_message_text(
                f"⏳ دانلود {i + 1}/{total} — {title[:40]}\n"
                f"{progress_bar} {progress_pct}%",
                reply_markup=cancel_keyboard,
            )

            with tempfile.TemporaryDirectory() as tmpdir:
                result = await yt_dlp_download(
                    url,
                    os.path.join(tmpdir, "%(id)s.%(ext)s"),
                    format_spec=format_id,
                )

                if "error" in result:
                    failed += 1
                    continue

                file_path = result["file_path"]
                if not os.path.exists(file_path):
                    failed += 1
                    continue

                file_size_mb = os.path.getsize(file_path) / (1024 * 1024)
                if file_size_mb > cfg["max_file_size_mb"]:
                    failed += 1
                    continue

                vtitle = result["title"] or title
                duration = format_duration(result.get("duration"))
                size = format_size(result.get("filesize"))
                caption = f"🎬 {vtitle} ({i + 1}/{total})"
                if duration and duration != "0:00":
                    caption += f"\n⏱ {duration}"
                caption += f"\n📦 {size}"

                with open(file_path, "rb") as f:
                    if file_size_mb <= 50:
                        await query.message.reply_video(
                            video=f,
                            caption=caption,
                            read_timeout=300,
                            write_timeout=300,
                        )
                    else:
                        await query.message.reply_document(
                            document=f,
                            caption=caption,
                            read_timeout=300,
                            write_timeout=300,
                        )
                increment_user_usage(user_id)
                success += 1

        except Exception:
            failed += 1

    await query.edit_message_text(
        f"✅ دانلود پلی‌لیست تمام شد!\n\n"
        f"✅ موفق: {success}/{total}\n"
        f"❌ ناموفق: {failed}/{total}"
    )


async def cb_cancel_download(update: Update, context: ContextTypes.DEFAULT_TYPE):
    query = update.callback_query
    await query.answer("🚫 در حال لغو...")
    context.user_data["cancel_download"] = True


async def post_init(application: Application):
    await application.bot.set_my_commands([
        BotCommand("start", "شروع / منوی اصلی"),
        BotCommand("help", "راهنما"),
        BotCommand("update", "آپدیت ربات (فقط مدیر)"),
    ])


def main():
    cfg = first_time_setup()

    print("Starting AparatKids Telegram Bot...")

    app = Application.builder().token(cfg["bot_token"]).post_init(post_init).build()

    app.add_handler(CommandHandler("start", cmd_start))
    app.add_handler(CommandHandler("help", cmd_help))
    app.add_handler(CommandHandler("update", cmd_update))
    app.add_handler(CallbackQueryHandler(cb_handler))
    app.add_handler(MessageHandler(filters.TEXT & ~filters.COMMAND, handle_message))

    print("Bot is running!")
    app.run_polling(allowed_updates=Update.ALL_TYPES)


if __name__ == "__main__":
    main()
