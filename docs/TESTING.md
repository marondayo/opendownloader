# Manual test checklist

Things automated tests and CLI runs can't cover: real desktops, real accounts, real clicks.
Tick them off per release. "Big video" = anything over ~500 MB (e.g. a long Bilibili concert).

## 1. Adding links

- [ ] Paste Link button and Ctrl+V with a YouTube `watch?v=…&list=…` link → 4 choices appear;
      each one does what it says (single video / whole playlist / pick videos / subscribe).
- [ ] Niconico mylist or series, Bilibili multi-part video, SoundCloud set → playlist dialog with
      thumbnails, durations, filter, Invert, select-all.
- [ ] Unsupported site (e.g. a Vimeo link) → "isn't supported yet" message, nothing else happens.
- [ ] Smart Mode on → pasting starts immediately with the preset; a playlist starts all entries.

## 2. Clipboard watching

- [ ] Copy a supported link in the browser → "link copied" card + taskbar flash → Download opens the dialog.
- [ ] Text that was on the clipboard when the app started is *not* offered.
- [ ] Right-click a download → Copy Link Address → the app does *not* offer it back.
- [ ] Copy a link that's already in the list → not offered.
- [ ] Settings → General → turn watching off → copying links does nothing.
- [ ] Smart Mode + "download copied links immediately" → copying starts the download.
- [ ] GNOME on Wayland and KDE/X11 if possible (clipboard access differs).

## 3. Downloads

- [ ] Video at several qualities (720p, 1080p, Best) and containers (MP4, MKV, WEBM on YouTube) → plays.
- [ ] Audio: MP3, M4A, OGG, OPUS, FLAC, WAV → plays, has cover art (not WAV) and tags.
- [ ] Subtitles: pick a language → embedded in MP4/MKV.
- [ ] Download the *same* video twice, then as audio only → all files kept (`Title (1).mp4`, `Title.m4a`).
- [ ] Pause / Resume / Retry; quit while downloading → confirm dialog; reopen → items paused, Resume continues.
- [ ] Playlist group: drill in, Pause all / Resume all, numbering `01.`, subfolder, `.m3u` (if enabled).

## 4. Bilibili

- [ ] Your previously slow video downloads at MB/s speeds (status shows "Finding the fastest Bilibili server…" first).
- [ ] Settings → Connection → Bilibili server: pick a fixed mirror, then "As provided by Bilibili" → both work.
- [ ] Sign out, then QR-code sign-in again (scan → "Scanned" → confirm on phone → "Signed in as …").

## 5. Danmaku & comments

- [ ] Bilibili video with lots of danmaku + "Save danmaku" → `Title.danmaku.ass` next to the video.
- [ ] Niconico video + "Save comments" → `Title.comments.ass`.
- [ ] Open the video in **mpv** and **VLC** → comments load automatically; check readability
      (font size, speed, density, colors, top/bottom comments).
- [ ] Settings → keep the original file → `.danmaku.xml` / `.comments.json` also saved.

## 6. Subscriptions

- [ ] Playlist dialog → 🔔 Subscribe with nothing ticked → button says "Subscribe" → appears in the Subscriptions tab.
- [ ] Check now → "0 new" (everything existing counts as seen).
- [ ] Subscribe to a channel that uploads often; after a new upload, Check now → it downloads into the same folder, notification shown.
- [ ] Pause / Resume / Open Folder / Copy Link / Unsubscribe.
- [ ] Settings → Downloads → interval (e.g. every hour) → leave the app open → it checks by itself.

## 7. Partial downloads & disk space

- [ ] Big video → right-click → **Remove and Delete Partial Download…** → confirm shows MB so far → files and
      `.odl-partial-*` folder are gone. Try on a running, a paused and a playlist item.
- [ ] Big video → **Remove from List** → Settings → Downloads → Leftover partial downloads → Scan finds it → Delete.
- [ ] Status bar shows free space; set "Always keep this much free" close to your free space → amber/red.
- [ ] With that limit: a download is refused/stopped with a clear message; Retry works after lowering the limit.
- [ ] Playlist dialog with a big playlist and a tight limit → "≈ … (estimated)" in red → Download shows the
      warning → "Choose fewer" goes back, "Download anyway" starts.

## 8. Media converter

- [ ] Drag files onto the window → converter opens with them; convert to each format; "Save next to original"
      and a custom folder.
- [ ] Right-click a finished download → Media Tools → Convert to MP3 → appears in the Converted tab.

## 9. Desktop integration

- [ ] Show in Folder (file is selected in the file manager), Open, Open Link in Browser, double-click to open.
- [ ] Desktop notifications (Settings → Notifications → Send a test notification).
- [ ] Browse… buttons (file/folder pickers) on GNOME and KDE.
- [ ] Japanese / Chinese / Korean titles render (install `fonts-noto-cjk` if not).

## 10. Packages

- [ ] **AppImage** on a real Linux desktop (not WSL): `chmod +x`, run, everything above works.
- [ ] **.deb**: `sudo apt install ./opendownloader_*.deb` → app menu entry with icon; uninstall cleanly.
- [ ] **tar.gz**: unpack → `./install.sh` → app in the menu (`~/.local`).
- [ ] **Windows installer**: installs without admin rights, Start-menu entry, optional desktop icon, launches
      after install, uninstalls from Settings → Apps. (Unsigned: SmartScreen shows "More info → Run anyway".)
- [ ] **Windows portable zip**: runs from any folder.

## 11. Windows app (first real run)

- [ ] First start → Install now → yt-dlp.exe, FFmpeg (zip) and Deno install; versions shown in Components.
- [ ] Downloads, conversions, danmaku work; **no console windows flash** when yt-dlp/ffmpeg run.
- [ ] Japanese titles render (Yu Gothic / Meiryo), file names with Japanese characters are fine.
- [ ] Show in Folder opens Explorer with the file selected; free space shows in the status bar.
- [ ] Known gap: desktop notifications are Linux-only for now.

## Later (not in scope yet)

- Account sign-in for each site (browser cookies, cookies.txt, Niconico password, SoundCloud token).
