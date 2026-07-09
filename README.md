# AparatKids Downloader

Desktop application for downloading videos from **AparatKids** and **YouTube** (and hundreds of other sites via yt-dlp).

Based on [Open Video Downloader](https://github.com/jely2002/youtube-dl-gui) with native AparatKids support.

---

## Supported Platforms

### AparatKids (Native Support)
- **URL Patterns:**
  - `https://www.aparatkids.com/w/{uid}` — Watch/episode pages
  - `https://www.aparatkids.com/m/{uid}` — Movie/series pages
- **Features:**
  - Direct HLS (m3u8) stream extraction
  - Persian and English title detection
  - Thumbnail and duration metadata
  - Subtitle track support (Persian/English)
  - Quality selection (720p, etc.)

### Aparat.com (Partial Support)
- **URL Patterns:**
  - `https://www.aparat.com/w/{uid}`
  - `https://www.aparat.com/m/{uid}`
- **Note:** Aparat.com loads video data via JavaScript, so extraction may not work for all videos. Use the AparatKids version of the video when available.

### YouTube (via yt-dlp)
- Videos, playlists, channels
- All YouTube URL formats supported
- Quality/format selection
- Subtitle and metadata download

### 1000+ Other Sites (via yt-dlp)
Any site supported by [yt-dlp](https://github.com/yt-dlp/yt-dlp#supported-sites), including:
- Vimeo, Dailymotion, Twitch
- TikTok, Instagram, Twitter/X
- Bilibili, NicoNico
- And many more

---

## Features

| Feature | Description |
|---------|-------------|
| **Cross-platform** | Windows, macOS, Linux |
| **Video + Audio** | Download full videos or extract audio only |
| **Quality selection** | Choose resolution, frame rate, format (MP4/MKV) |
| **Playlists** | Download entire playlists in one go |
| **Subtitles** | Auto-download available captions |
| **Metadata** | Title, thumbnail, duration, description |
| **Custom output** | Set download location and filename templates |
| **Smart queueing** | Automatic download balancing |
| **Authentication** | Browser cookies, basic auth, video passwords |
| **Auto updates** | App and yt-dlp kept up to date |
| **Dark/Light mode** | Adapts to system theme |
| **Keyboard shortcuts** | Quick queue and download actions |

---

## Download

Download the latest release from the [Releases page](https://github.com/ScannerVpn/Downloader/releases).

| Platform | File |
|----------|------|
| **Windows (x64)** | `AparatKids-Downloader_*_x64-setup.exe` |
| **macOS (Intel)** | `AparatKids-Downloader_*_x64.dmg` |
| **macOS (Apple Silicon)** | `AparatKids-Downloader_*_aarch64.dmg` |
| **Linux (x64 AppImage)** | `AparatKids-Downloader_*_amd64.AppImage` |
| **Linux (aarch64 AppImage)** | `AparatKids-Downloader_*_aarch64.AppImage` |
| **Linux (Debian/Ubuntu x64)** | `AparatKids-Downloader_*_amd64.deb` |
| **Linux (Fedora/RHEL x64)** | `AparatKids-Downloader_*_amd64.rpm` |

---

## How It Works

```
┌─────────────────────────────────────────────────┐
│  User pastes URL                                │
└────────────────────┬────────────────────────────┘
                     │
          ┌──────────▼──────────┐
          │  Is it AparatKids?  │
          └──┬───────────────┬──┘
             │ YES           │ NO
    ┌────────▼────────┐  ┌──▼──────────────┐
    │ Rust resolver   │  │ yt-dlp handles  │
    │ fetches page    │  │ the URL         │
    │ extracts m3u8   │  │ directly        │
    │ + metadata      │  │                 │
    └────────┬────────┘  └──┬──────────────┘
             │              │
          ┌──▼──────────────▼──┐
          │  yt-dlp downloads  │
          │  the video stream  │
          └────────────────────┘
```

### AparatKids Extraction Flow
1. Detects `aparatkids.com` URL pattern
2. Fetches the page HTML via HTTP
3. Parses the `player_data` JavaScript object
4. Extracts the HLS m3u8 stream URL
5. Extracts metadata (title, thumbnail, duration) from `uxEvents.movie`
6. Passes the m3u8 URL to yt-dlp for downloading

---

## Development

### Prerequisites
- Node.js v24+
- Rust (latest stable)
- Tauri CLI

### Setup
```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

### Build Requirements (Windows)
The `libsodium` library is required for the keyring plugin:
```bash
# Download prebuilt libsodium
# Set environment variables before building:
$env:SODIUM_LIB_DIR = "path\to\libsodium\x64\Release\v143\dynamic"
$env:SODIUM_INCLUDE_DIR = "path\to\libsodium\include"
```

### Project Structure
```
src/                          # Vue 3 Frontend
  components/                 # UI components
  composables/                # Vue composition utilities
  helpers/                    # Utility functions
  stores/                     # Pinia state management
  tauri/                      # Tauri bridge layer

src-tauri/                    # Rust Backend
  src/
    runners/
      aparatkids.rs           # AparatKids URL resolver (NEW)
      ytdlp_runner.rs         # yt-dlp process management
      ytdlp_info.rs           # Video metadata fetcher
      ytdlp_download.rs       # Video downloader
    parsers/                  # yt-dlp output parsing
    models/                   # Data models
    scheduling/               # Task scheduling & pipelines
    commands/                 # Tauri IPC commands
```

---

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Frontend | Vue 3, TypeScript, Tailwind CSS, Pinia |
| Backend | Rust, Tauri v2 |
| Video Engine | yt-dlp |
| AparatKids Extractor | Custom Rust HTTP resolver |

---

## License

[AGPL-3.0](./LICENSE)

---

## Disclaimer

This software is provided for educational purposes. Users are responsible for complying with applicable laws and platform terms of service.
