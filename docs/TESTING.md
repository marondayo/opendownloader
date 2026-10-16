# Manual test checklist

Things automated tests and CLI runs can't cover: real desktops, real accounts, real clicks.
Tick them off per release. "Big video" = anything over ~500 MB (e.g. a long Bilibili concert).

## 1. Adding links

- [x] Paste Link button and Ctrl+V with a YouTube `watch?v=…&list=…` link → 4 choices appear;
      each one does what it says (single video / whole playlist / pick videos / subscribe).
- [x] Niconico mylist or series, Bilibili multi-part video, SoundCloud set → playlist dialog with
      thumbnails, durations, filter, Invert, select-all.
- [x] Unsupported site (e.g. a Vimeo link) → "isn't supported yet" message, nothing else happens.
- [x] Smart Mode on → pasting starts immediately with the preset; a playlist starts all entries.

## 2. Clipboard watching

- [x] Copy a supported link in the browser → "link copied" card + taskbar flash → Download opens the dialog.
- [x] Text that was on the clipboard when the app started is *not* offered.
- [x] Right-click a download → Copy Link Address → the app does *not* offer it back.
- [x] Copy a link that's already in the list → not offered.
- [x] Settings → General → turn watching off → copying links does nothing.
- [x] Smart Mode + "download copied links immediately" → copying starts the download.
- [x] GNOME on Wayland and KDE/X11 if possible (clipboard access differs).

## 3. Downloads

- [x] Video at several qualities (720p, 1080p, Best) and containers (MP4, MKV, WEBM on YouTube) → plays.
- [x] Audio: MP3, M4A, OGG, OPUS, FLAC, WAV → plays, has cover art (not WAV) and tags.
- [x] Subtitles: pick a language → embedded in MP4/MKV.
- [x] Download the *same* video twice, then as audio only → all files kept (`Title (1).mp4`, `Title.m4a`).
- [x] Pause / Resume / Retry; quit while downloading → confirm dialog; reopen → items paused, Resume continues.
- [X] Playlist group: drill in, Pause all / Resume all, numbering `01.`, subfolder, `.m3u` (if enabled).

## 4. Bilibili

- [x] Your previously slow video downloads at MB/s speeds (status shows "Finding the fastest Bilibili server…" first).
- [x] Settings → Connection → Bilibili server: pick a fixed mirror, then "As provided by Bilibili" → both work.
- [x] Sign out, then QR-code sign-in again (scan → "Scanned" → confirm on phone → "Signed in as …").

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

- [x] Big video → right-click → **Remove and Delete Partial Download…** → confirm shows MB so far → files and
      `.odl-partial-*` folder are gone. Try on a running, a paused and a playlist item.
- [x] Big video → **Remove from List** → Settings → Downloads → Leftover partial downloads → Scan finds it → Delete.
- [x] Status bar shows free space; set "Always keep this much free" close to your free space → amber/red.
- [x] With that limit: a download is refused/stopped with a clear message; Retry works after lowering the limit.
- [x] Playlist dialog with a big playlist and a tight limit → "≈ … (estimated)" in red → Download shows the
      warning → "Choose fewer" goes back, "Download anyway" starts.

## 8. Media converter

- [x] Drag files onto the window → converter opens with them; convert to each format; "Save next to original"
      and a custom folder.
- [x] Right-click a finished download → Media Tools → Convert to MP3 → appears in the Converted tab.

## 9. Desktop integration

- [x] Show in Folder (file is selected in the file manager), Open, Open Link in Browser, double-click to open.
- [x] Desktop notifications (Settings → Notifications → Send a test notification).
- [x] Browse… buttons (file/folder pickers) on GNOME and KDE.
- [x] Japanese / Chinese / Korean titles render (install `fonts-noto-cjk` if not).

## 10. Packages

- [x] **AppImage** on a real Linux desktop (not WSL): `chmod +x`, run, everything above works.
- [x] **.deb**: `sudo apt install ./opendownloader_*.deb` → app menu entry with icon; uninstall cleanly.
- [x] **tar.gz**: unpack → `./install.sh` → app in the menu (`~/.local`).
- [x] **Windows installer**: installs without admin rights, Start-menu entry, optional desktop icon, launches
      after install, uninstalls from Settings → Apps. (Unsigned: SmartScreen shows "More info → Run anyway".)
- [x] **Windows portable zip**: runs from any folder.

## 11. Windows app (first real run)

- [x] First start → Install now → yt-dlp.exe, FFmpeg (zip) and Deno install; versions shown in Components.
- [x] Downloads, conversions, danmaku work; **no console windows flash** when yt-dlp/ffmpeg run.
- [x] Japanese titles render (Yu Gothic / Meiryo), file names with Japanese characters are fine.
- [x] Show in Folder opens Explorer with the file selected; free space shows in the status bar.
- [ ] Known gap: desktop notifications are Linux-only for now.

## 12. Accounts

Settings → Accounts (or the 👤 button). Bilibili QR sign-in is covered in §4. For each site, test with
something that needs an account (age-restricted or members-only video, Niconico premium-only quality,
SoundCloud Go+ track) and check the status line next to the site name.

- [ ] Not signed in → a download that needs an account fails with "Tip: sign in to … in Settings → Accounts."
- [ ] **Cookies from browser**, Firefox: sign in to the site in Firefox → pick Firefox → the download works.
      Repeat with a non-default **Profile** name.
- [ ] **Cookies from browser**, Chrome/Chromium: keyring prompt on GNOME/KDE is handled; close the
      browser if the cookie database is locked. On Windows, note whether Chrome/Edge cookies can be read.
- [ ] **cookies.txt**: export one while signed in → Choose cookies.txt… → Check shows "✔ Login session
      found" → download works.
- [ ] **cookies.txt** checks: a file for a different site → "⚠ No … login cookie found"; a signed-out or
      expired export → "✖ … has expired"; delete the file → status shows "cookies.txt missing" and a
      download fails with "cookie file … is missing".
- [ ] **cookies.txt** is left unchanged by downloads (yt-dlp works on a private copy), also with several
      downloads running at once.
- [ ] **Niconico, Account login**: email + password → Save → "Signed in as …" → premium/login-only video
      works. A wrong password gives a clear error, not a hang.
- [ ] **Niconico** comments ("Save comments") still work while signed in with each method.
- [ ] **SoundCloud, Account login**: paste the `oauth_token` cookie value → Save → "OAuth token saved" → a
      Go+ track downloads at the higher quality.
- [ ] **Bilibili** with browser cookies and with cookies.txt → above-480p quality available.
- [ ] Forget / Sign out → status back to "Not signed in"; the private netrc entry and managed cookie
      file are gone; restarting the app keeps that state.
- [ ] Settings → Accounts survives a restart for every method (browser, profile, cookies.txt path).
