# OpenDownloader

A free, open-source desktop app for downloading video and audio from **YouTube**, **Niconico**,
**Bilibili** and **SoundCloud**, plus a built-in audio/video converter. Written in Rust with
[egui](https://github.com/emilk/egui); downloads are powered by [yt-dlp](https://github.com/yt-dlp/yt-dlp)
and [FFmpeg](https://ffmpeg.org).

> Status: **v0.1, Linux first.** The code is cross-platform and Windows support is planned next.

## Features

- **Paste Link** (or Ctrl+V anywhere): the link is analyzed and a download dialog opens.
- **Playlists done right**: for a YouTube `watch?v=…&list=…` link, choose
  - *Download the Video*: only the `v=` video,
  - *Download the Playlist*: every video,
  - *Choose from the Playlist*: tick exactly the videos you want (with filter, select all, invert).
- Playlists, Niconico mylists/series, Bilibili multi-part videos/bangumi/favorites, SoundCloud sets and
  profiles download as a **group** with per-item progress, optional subfolder, `01.` numbering and `.m3u`.
- **Video**: MP4 / MKV / WEBM, 144p → 8K / Best, codec preference (H.264, VP9, AV1), subtitles
  (embedded), chapters, thumbnails and metadata, optional SponsorBlock.
- **Audio**: MP3, M4A, OGG (Vorbis), OPUS, FLAC, WAV, with bitrate choice and cover art.
- **Media Tools**: convert downloaded or local files (drag & drop onto the window) to MP3, M4A, OGG, OPUS,
  FLAC, WAV, MP4 (H.264) or MKV.
- **Accounts** per site:
  - YouTube: cookies from your browser, or a cookies.txt file
  - Niconico: email + password (stored in a private netrc), browser cookies or cookies.txt
  - Bilibili: **native QR code login** (scan with the Bilibili app), browser cookies or cookies.txt
  - SoundCloud: OAuth token, browser cookies or cookies.txt
- **Subscriptions**: subscribe to a playlist, channel, mylist or favorites list and new videos download
  automatically (checked every 1–24 h, one subscription at a time).
- **Clipboard watching**: copy a supported link anywhere and the app offers to download it.
- **Danmaku & comments**: save Bilibili danmaku and Niconico comments as `.ass` subtitles next to the video
  (scrolling/top/bottom, colors and sizes kept; mpv and VLC load them automatically).
- **Fast Bilibili downloads overseas**: the fastest official CDN mirror is picked before each download.
- Downloads never touch existing files: each runs in a private working folder and the result gets a free name.
- **Smart Mode**: one-click downloads with your preset.
- Pause / resume / retry, simultaneous downloads (1–8), fragment threads, speed limit, proxy
  (HTTP/HTTPS/SOCKS5), download archive (never download twice), desktop notifications.
- Context menu: Open, Show in Folder, Copy Link Address, Open Link in Browser, Paste Link, Pause/Resume/Retry,
  Download Again, Media Tools ▸ Convert to…, Remove, Delete File, Clear List.
- **Components manager**: installs and auto-updates private copies of yt-dlp, FFmpeg and Deno
  (the JavaScript runtime yt-dlp uses for YouTube). No root required.
- History is saved; unfinished downloads come back paused and resume where they stopped.

## Install / build (Linux)

Requirements: Rust **1.95+**, and the usual windowing libraries:

```sh
# Debian/Ubuntu
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev libgl1-mesa-dev
# Optional but recommended for Japanese/Chinese/Korean titles
sudo apt install fonts-noto-cjk
```

```sh
cargo build --release -p odl-gui
packaging/linux/install.sh            # installs to ~/.local (binary, .desktop file, icon)
opendownloader                        # or: opendownloader "https://www.youtube.com/watch?v=…"
```

On first start, click **Install now** in the banner (or Settings → Components) to fetch yt-dlp and Deno.
FFmpeg can come from your package manager (`sudo apt install ffmpeg`) or be installed by the app.

## Command-line engine

`odl-core` has a small CLI example, useful for testing without the GUI:

```sh
cargo run -p odl-core --example odl-cli -- install           # install missing components
cargo run -p odl-core --example odl-cli -- tools             # show versions
cargo run -p odl-core --example odl-cli -- probe <url>
cargo run -p odl-core --example odl-cli -- get <url> mp3 ~/Music
```

## Project layout

```text
crates/odl-core   engine: link parsing, yt-dlp/ffmpeg orchestration, queue, accounts, components
crates/odl-gui    the desktop app (egui/eframe), binary name `opendownloader`
packaging/linux   .desktop file, icon, user install script
```

Data lives in the XDG directories (`~/.config/opendownloader`, `~/.local/share/opendownloader`).
Set `OPENDOWNLOADER_HOME=/some/dir` for a portable setup.

## Notes on accounts and privacy

Credentials never leave your machine except to the site itself, through yt-dlp. Passwords and tokens are
stored in `~/.config/opendownloader/netrc` (mode 600); QR-login cookies in
`~/.config/opendownloader/cookies/` (mode 600). Each download gets a private temporary copy of the cookie
file, so parallel downloads never corrupt it.

Bilibili blocks anonymous requests from some networks (HTTP 412). Signing in with the QR code fixes that.

## Thanks

yt-dlp, FFmpeg, Deno and egui. Projects that inspired the site support:
[nico_downloader](https://github.com/masteralice3104/nico_downloader),
[bilibili-video-downloader](https://github.com/lanyeeee/bilibili-video-downloader),
[B23Downloader](https://github.com/vooidzero/B23Downloader).

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

Download only content you have the right to download, and respect each site's terms of service.
