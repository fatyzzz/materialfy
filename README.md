<p align="center"><b>English</b> · <a href="README.ru.md">Русский</a> · <a href="README.pt-BR.md">Português</a></p>

<div align="center">

<h1>
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/brand/wordmark-on-dark.svg">
  <img src="docs/brand/wordmark.svg" width="420" alt="Materialfy">
</picture>
</h1>

[![Release build](https://github.com/makdren/materialfy/actions/workflows/release.yml/badge.svg)](https://github.com/makdren/materialfy/actions/workflows/release.yml)
[![Release](https://img.shields.io/github/v/release/makdren/materialfy?label=release)](https://github.com/makdren/materialfy/releases/latest)
[![Build](https://github.com/makdren/materialfy/actions/workflows/build.yml/badge.svg)](https://github.com/makdren/materialfy/actions/workflows/build.yml)

Spotify client for Android built with Rust and Qt Quick, with a Material 3 Expressive interface.<br>
A phone port of the Materialfy desktop app.

[Download](#download) ·
[Android](#android) ·
[Screenshots](#screenshots) ·
[Features](#features) ·
[Limitations](#whats-missing-and-why) ·
[Installation](#installation) ·
[Building](#building-from-source) ·
[Data and privacy](#where-data-is-stored)

</div>

<br>

Materialfy replaces the official Spotify desktop app for listening to music in a regular native window. The
interface is written in QML (Qt 6.12) with no browser engine, and the logic is in Rust. Audio is played by
[librespot](https://github.com/librespot-org/librespot) inside the app's own process, so playback on the computer
requires Spotify Premium. Interface colors follow the cover of the current track. The interface follows the system
language: Russian on a Russian system, English otherwise; Settings → Language can fix either one.

The project is assembled from two MIT-licensed projects: the Spotify side comes from
[spotifast](https://github.com/crmne/spotifast) (crmne), the interface from
[yamusic-md3](https://github.com/yaskorinov/yamusic-md3) (yaskorinov). See
[Origin and license](#origin-and-license) for details.

> [!NOTE]
> This is a fork of [fatyzzz/materialfy](https://github.com/fatyzzz/materialfy) with an Android version: the same
> app with a phone layout, background playback, the player in the system "island" and APK updates from inside
> the app. Only the APK is released here; Windows, Linux and macOS builds are in the
> [upstream releases](https://github.com/fatyzzz/materialfy/releases).

## Download

The APK is in the [latest release](https://github.com/makdren/materialfy/releases/latest) (every version is on the [Releases page](https://github.com/makdren/materialfy/releases)):

| System | File |
| --- | --- |
| Android 9+, arm64 | `Materialfy-<version>-arm64-v8a.apk` |

`SHA256SUMS.txt` in the same release lists the checksum. The app checks the releases for new versions on its own
(Settings → Updates): the new APK is downloaded, checked against the checksum and handed to the system installer.
Desktop builds and their updates are in [fatyzzz/materialfy](https://github.com/fatyzzz/materialfy/releases).

## Android

<p align="center"><img src="docs/screenshots/android/phone-en.png" width="100%" alt="Materialfy on a phone: home, album, now playing, track menu"></p>
<p align="center"><sub>Home, album, Now Playing and the track menu. Captured with test data in a 412×915 window.</sub></p>

The phone gets the same features as the desktop, in a one-handed layout: bottom navigation, a mini player above
it, a full-screen player (swipe down to close), menus as bottom sheets, long press instead of right click, and
the system Back button.

- Music keeps playing in the background: a media notification with cover and seeking, the lock screen, headset and
  Bluetooth buttons, pausing for calls and when headphones are unplugged.
- The player in the system "island": on Android 16 as a Live Update (status bar chip, Samsung Now Bar, HyperOS
  Super Island, ColorOS Fluid Cloud), on vivo in the OriginOS Atomic Island. It can be turned off in Settings →
  Playback.
- The phone shows up in Spotify Connect as a smartphone, so music can move between it and the computer.
- The "Listening to Spotify" status in Discord comes from the Spotify ↔ Discord connection and works the same
  on the phone.
- Updates: Settings → Updates downloads the APK from the latest release and installs it over the current one.

**Installing.** Download `Materialfy-<version>-arm64-v8a.apk` from the [latest release](https://github.com/makdren/materialfy/releases/latest)
and open it. Android asks once to allow installs from the browser or file manager. If a test build signed with a
different key (`-debugsigned.apk`) is installed, uninstall it first: the release APK cannot be installed over it.
Sign-in works as on the desktop: the browser opens, and after signing in you return to the app.

You need an arm64 phone with Android 9 or later, and Spotify Premium for audio. The Android version is still
experimental: please report problems in [Issues](https://github.com/makdren/materialfy/issues), ideally with the
phone model and Android version.

## Screenshots

Taken with a Premium account and music playing; the colours follow the cover of the current track.

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/materialfy/home-dark.png" alt="Home, dark theme"></td>
    <td width="50%"><img src="docs/screenshots/materialfy/home-light.png" alt="Home, light theme"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/materialfy/now-playing.png" alt="Now Playing with synced lyrics"></td>
    <td><img src="docs/screenshots/materialfy/artist.png" alt="Artist page"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/materialfy/releases.png" alt="New releases"></td>
    <td><img src="docs/screenshots/materialfy/settings.png" alt="Settings"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/materialfy/account.png" alt="Spotify account"></td>
    <td><img src="docs/screenshots/materialfy/playback.png" alt="Playback on this computer"></td>
  </tr>
  <tr>
    <td><img src="docs/screenshots/materialfy/compact-900x640.png" alt="900×640 window with a narrow sidebar"></td>
    <td><img src="docs/screenshots/materialfy/compact-640x720.png" alt="Compact layout, 640×720"></td>
  </tr>
  <tr>
    <td valign="top"><img src="docs/screenshots/materialfy/mini-300x120.png" alt="Mini player, 300×120"></td>
    <td valign="top"><sub>The mini player: the window switches to it when it is narrower than 480 or shorter than
      400 pixels.</sub></td>
  </tr>
</table>

## Features

- Sign-in through the browser with OAuth PKCE. You type your password on the Spotify website; the app only
  receives a one-time code on `127.0.0.1`.
- Playback on this computer through librespot (Premium required): 96, 160 or 320 kbit/s, volume normalization,
  gapless playback, crossfade up to 12 seconds, speed from 0.5× to 2× without changing pitch, similar music after
  an album or playlist ends, output device selection, and an audio cache from 256 MB to 10 GB.
- The computer shows up in Spotify Connect under the name set in Settings. You can move playback to a phone or a
  speaker and back; while music plays on another device, the window works as a remote with that device's
  volume. Speakers on the local network that are not signed in yet are found over mDNS and can be connected
  from the devices menu.
- Queue: a manual queue kept separate from the context, "Play next", dragging tracks into the queue and
  reordering rows, removal with undo.
- Radio from a track, artist, album or playlist, and from any track in search results. A station can be
  regenerated or saved as a playlist.
- Library: playlists with folders, pinning to the top (in this app only), Liked Songs, albums, artists,
  podcasts. Grid or list view, sorting.
- Editing your own playlists: name, description, cover, public or private, reordering tracks with the mouse,
  removal with an Undo button, adding tracks from recent, liked and search, creating and deleting playlists.
- Search across tracks, albums, artists, playlists and podcasts. An `open.spotify.com` link can be pasted or
  dropped onto the window.
- Artist page with monthly listeners, biography, popular tracks and releases. The New releases page collects
  recent releases from artists you follow.
- Podcasts: show and episode pages, listening progress, chapters, skip back 10 seconds and forward 30.
- Synced lyrics. The app takes lyrics from Spotify first and falls back to LRCLIB, NetEase and Musixmatch.
  Three styles to choose from (classic, Spicy, karaoke), text size
  and background scrim.
- Material You color scheme from the current track's cover or from a chosen accent color, light and dark themes.
  Animations can be shortened or follow the system setting.
- Three window layouts: full, compact for a narrow window, and a mini player with an "Always on top" option
  (not available on Wayland).
- Its own window frame on every OS: Material 3 minimize, maximize and close buttons, rounded corners (on Windows 11
  drawn by the system; Windows 10 keeps square ones), square corners when maximized.
- Media keys, the SMTC tile on Windows and MPRIS on Linux, a tray icon with minimize-on-close, single running
  instance.
- Interface in English, Russian or Portuguese (Brazil). By default it follows the system language; Settings → Language switches it.
- Proxy: system, HTTP or SOCKS5, with username and password. Sign-in, Web API, playback, lyrics and images all
  go through it.

## What's missing and why

- librespot only works with Premium accounts. With a free account music does not play on the computer, and
  radio, which is built through librespot, is blocked. The Spotify Web API also allows controlling other devices
  only with Premium. Without it you still have the library, search and playlist editing.
- Spotify serves librespot up to 320 kbit/s Ogg Vorbis, so there is no lossless.
- Crossfade and speed only work when playing on this computer. They are not passed to a speaker or phone over
  Connect, and other Spotify devices see the track position with corrections once per second.
- Tracks cannot be downloaded for offline listening; the app does not bypass DRM. The audio cache only speeds
  up repeat playback.
- The shared Web API Client ID is one for all users of the app, and Spotify often rate-limits it. That is why
  the app goes through your Spotify session first and keeps the Web API as a fallback. Details and how to set up
  your own Client ID are in [Spotify session and Web API](#spotify-session-and-web-api).
- Some limits come from the Web API itself. The context queue shows about 20 upcoming tracks, because the API
  does not return the total. Removing a track from a playlist removes all of its duplicates, because the delete
  method does not accept a position. Spotify history is limited to the last 50 plays.
- The app is unofficial. Spotify may restrict third-party clients, and you are responsible for how your account
  is used.

## Requirements

- A Spotify account. Premium is needed for audio on this computer and for radio.
- Windows 10 1809 or later (x64); Linux x86_64 or aarch64; macOS 13 or later (the build is experimental; media
  keys on macOS are untested); Android 9 or later (arm64).
- Free local ports 8989 and 8898: Spotify sends the browser back to them after sign-in. If the firewall asks,
  allow connections on `127.0.0.1`.

## Installation

The quickest way is a ready build from [Download](#download). To build from source instead:

### Windows

You need Rust, Qt 6.12 and Build Tools for Visual Studio (details in [Building](#windows-1)). The script builds
the app and the installer:

```powershell
git clone https://github.com/makdren/materialfy
cd materialfy
.\build_windows.ps1 -QtDir C:\Qt\6.12.0\msvc2022_64
```

`target\dist\` will contain `Materialfy-<version>-Setup.exe` and a portable `materialfy\` folder. The installer
does not need administrator rights and installs into `%LocalAppData%\Programs\Materialfy`.

### Linux

```bash
git clone https://github.com/makdren/materialfy
cd materialfy
./install.sh
```

The script works with apt, pacman, dnf and zypper. It first shows which system packages it will install with
sudo and waits for confirmation. Rust is installed with rustup into `~/.cargo`. If the system has no Qt 6.12
with QML modules, the script downloads one next to the sources, about 250 MB. The first build takes 5 to 15
minutes; after that you get the `~/.local/bin/materialfy` command and an entry in the application menu. Running
the script again updates the app.

```bash
./install.sh --uninstall   # remove the command and the menu entry; settings, data and sign-in are kept
```

### macOS

There is no installer; build the `.app` as described in [Building](#macos-experimental).

### Android

The release APK is covered in [Android](#android); building the APK from source is in
[Building](#android-experimental).

## First run

Spotify grants two separate permissions, and the app asks for them one at a time.

1. Account sign-in: Account → Sign in with Spotify. A browser opens, and after you confirm, Spotify sends it
   back to `http://127.0.0.1:8989/login`. This is enough for the library, search and playlists.
2. Playback on this computer: the Playback screen or the devices menu. The second confirmation in the browser
   comes back to `http://127.0.0.1:8898/login`. After that the computer appears in Spotify Connect.

Without the second step Materialfy works as a remote for other devices (with Premium).

## Spotify session and Web API

The app has two ways to get Spotify data.

- The librespot session. This is the same protocol the official app uses; it becomes available after the
  second sign-in step (playback on this computer). The session runs under your account, and Web API quotas do
  not apply to it.
- The Web API (`api.spotify.com`) through a Client ID. By default this is the app's shared Client ID, one for all
  of its users. Spotify often answers it with 429 ("too many requests") and sometimes with `QUOTA_EXCEEDED`, in
  which case the Web API does not respond at all until the quota resets, which can take up to a day.

Reads go through the session first; the Web API is asked only when there is no session, it did not answer in
time, or it does not support that request. The session currently serves playlists (the list, their contents,
editing tracks), Liked Songs, saved albums and artists, album, artist and track pages, search and New releases.
Still Web API only: podcasts in the library and their pages, listening history, the home shelves "Top" and
"Made for you", radio seed labels, creating a playlist, changing its name or cover, following someone else's
playlist, and remote control of another device (commands and polling its state).

To make the shared Client ID last longer, the Web API client:

- splits requests into three queues: the page you opened, background refresh, and player state polling.
  Background gets up to two thirds of the budget (the shared key allows 40 requests per 30 seconds), polling a
  quarter, pages all of it;
- merges identical requests already in flight and keeps GET responses for 20 seconds (any write clears the
  cache and keeps it from filling for a minute);
- after a 429 or an exhausted quota, keeps background and polling off the network, while pages retry every 15
  seconds, or every minute for a quota. Player polling waits longer after each consecutive 429: one minute, two,
  four, up to ten minutes.

While a limit is active, Settings → Account shows a "Web API limited" notice with an estimated end time.

### Your own Client ID

If the shared key keeps hitting limits, register your own Spotify app. Its limits are shared only with you.

1. Open [developer.spotify.com/dashboard](https://developer.spotify.com/dashboard), sign in and click
   "Create app".
2. Any name and description will do. Add `http://127.0.0.1:8989/login` to Redirect URIs and tick Web API in
   the API section. Save.
3. A new app runs in development mode and only lets in users from its list. Add the email of your Spotify
   account under "User Management".
4. Copy the Client ID (32 characters) from the app page.
5. In Materialfy: Settings → Account → Personal Spotify app → Set up. The form shows the redirect URI from step 2
   with a copy button. Paste the Client ID, click Save and confirm access in the browser.

Even with your own Client ID, some Web API requests go through the shared one: other people's playlists, the
list of your playlists, playlist search, and popular tracks and related artists on the artist page.

## Where data is stored

| What | Windows | Linux | macOS |
|---|---|---|---|
| Settings | registry `HKCU\Software\materialfy\Materialfy` | `~/.config/materialfy/Materialfy.conf` | `~/Library/Preferences/materialfy/Materialfy.conf` |
| Data (player session, history) | `%APPDATA%\materialfy\Materialfy` | `~/.local/share/materialfy/Materialfy` | `~/Library/Application Support/materialfy/Materialfy` |
| Cache (audio, covers, lyrics) | `%LOCALAPPDATA%\materialfy\Materialfy\cache` | `~/.cache/materialfy/Materialfy` | `~/Library/Caches/materialfy/Materialfy` |
| Tokens and proxy password | Credential Manager | Secret Service (keyring) | Keychain |

Secrets are kept only in the system store under the name `materialfy`. Sign out in Settings removes all Spotify
permissions from the store. The cache stays; it can be cleared in Settings → Storage.

## Privacy

The app has no telemetry or usage statistics of its own. Network requests go to Spotify servers (sign-in, Web
API, playback, covers) and to the lyrics services LRCLIB, NetEase and Musixmatch. On the local network the app
looks for Spotify Connect devices over mDNS.

You enter your Spotify password only on `accounts.spotify.com` in your own browser. The app requests access to
the library, playlists and playback control, and reads your plan (Free or Premium) through `user-read-private`.
It does not ask for access to your email.

## Keyboard shortcuts

| Keys | Action |
|---|---|
| <kbd>Space</kbd> | play or pause |
| <kbd>Ctrl</kbd> + <kbd>→</kbd> / <kbd>←</kbd>, media keys | next or previous track |
| <kbd>Ctrl</kbd> + <kbd>↑</kbd> / <kbd>↓</kbd> | volume |
| <kbd>Shift</kbd> + <kbd>→</kbd> / <kbd>←</kbd> | seek 10 seconds |
| <kbd>Ctrl</kbd> + <kbd>P</kbd> | full-screen player |
| <kbd>Ctrl</kbd> + <kbd>L</kbd> | lyrics |
| <kbd>Ctrl</kbd> + <kbd>F</kbd> | search in the list; press again for global search |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>L</kbd> | library |
| <kbd>Ctrl</kbd> + <kbd>B</kbd> | collapse or expand the sidebar |
| <kbd>Ctrl</kbd> + <kbd>,</kbd> | settings |
| <kbd>Alt</kbd> + <kbd>←</kbd>, mouse back button | back |
| <kbd>F11</kbd> | full screen |
| <kbd>Ctrl</kbd> + <kbd>Q</kbd> | quit on Linux (<kbd>⌘</kbd> + <kbd>Q</kbd> on macOS) |

## Building from source

Common requirements:

- Rust 1.98 or later ([rustup](https://rustup.rs); on Windows the `stable-x86_64-pc-windows-msvc` toolchain).
- Qt 6.12 with the qtdeclarative, qtsvg, qtshadertools and qtimageformats modules. It can be installed with the
  Qt Online Installer or with [`install_qt.py`](.github/scripts/install_qt.py), the script CI uses: it downloads
  archives from download.qt.io and verifies checksums.
- Python 3 for `install_qt.py`.

cxx-qt finds Qt through the `QMAKE` variable (the path to `qmake6`). The built app looks for QML in this order:
`$MATERIALFY_QML_DIR`, then `qml/` next to the executable, then `src/materialfy/qml` in the repository, so
`cargo run` works from the source tree.

Build with `--locked`. `Cargo.lock` pins `vergen 9.0.6`; version 9.1 breaks the `librespot-core` build script.
Do not run `cargo update` without `-p`.

### Windows

You need [Build Tools for Visual Studio 2022](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with
the "Desktop development with C++" workload, Rust (msvc), Qt 6.12 `msvc2022_64`, PowerShell 7, and, for the
installer, [Inno Setup 6](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup`).

```powershell
python .github\scripts\install_qt.py --out C:\Qt\6.12.0\msvc2022_64   # if Qt is not installed yet
.\build_windows.ps1 -QtDir C:\Qt\6.12.0\msvc2022_64
```

[`build_windows.ps1`](build_windows.ps1) runs `cargo build --locked --release -p materialfy`, assembles the exe,
`qml\`, `assets\`, the Qt libraries (through windeployqt) and the MSVC runtime in `target\dist\materialfy\`, then
calls Inno Setup if it finds it. Without `-QtDir`, the script looks for Qt in `$env:QTDIR`, next to
`$env:QMAKE`, in `C:\Qt\6.12.0\msvc2022_64` and in `~\Qt\6.12.0\msvc2022_64`.

| Flag | What it does |
|---|---|
| `-SkipInstaller` | skip the Inno Setup step |
| `-Portable` | also produce `Materialfy-<version>-windows-x64-portable.zip` |
| `-SkipBuild` | use an existing `target\<profile>\materialfy.exe` |
| `-CargoProfile debug` | debug build |
| `-Version 1.2.3` | installer version (taken from `Cargo.toml` by default) |
| `-OutDir DIR` | output directory (`target\dist` by default) |

For development without packaging:

```powershell
.\run_windows.bat --new-instance      # cargo run --release with the Qt it finds; arguments go to the app
```

or by hand:

```powershell
$env:QMAKE = "C:\Qt\6.12.0\msvc2022_64\bin\qmake6.exe"
$env:PATH = "C:\Qt\6.12.0\msvc2022_64\bin;$env:PATH"
cargo run --locked -p materialfy
```

### Linux

The easiest way is `./install.sh` (see [Installation](#linux)). By hand on Debian or Ubuntu:

```bash
sudo apt install build-essential pkg-config lld python3 7zip libasound2-dev libpulse-dev \
    libgl-dev libegl-dev libxkbcommon-dev libfontconfig1-dev libdbus-1-dev libxcb-cursor0 libxkbcommon-x11-0

python3 .github/scripts/install_qt.py --out ~/Qt/6.12.0/gcc_64
export QMAKE=~/Qt/6.12.0/gcc_64/bin/qmake6 LD_LIBRARY_PATH=~/Qt/6.12.0/gcc_64/lib

cargo run --locked --release -p materialfy
```

On older Ubuntu releases the `7zip` package is called `p7zip-full`. On Arch:
`sudo pacman -S base-devel lld alsa-lib libpulse 7zip`. cxx-qt does not link with GNU ld.bfd; you need `lld`,
`mold` or `gold`.

The AppImage and `tar.gz` are built by [`package_linux.sh`](.github/scripts/package_linux.sh); the output goes to
`target/dist/`:

```bash
bash .github/scripts/package_linux.sh 0.1.0
```

### macOS (experimental)

```bash
brew install sevenzip
python3 .github/scripts/install_qt.py --host mac --out ~/Qt/6.12.0/macos
export QMAKE=~/Qt/6.12.0/macos/bin/qmake6
cargo build --locked --release -p materialfy
bash .github/scripts/package_macos.sh 0.1.0   # target/dist/Materialfy.app and .dmg
```

### Android (experimental)

The same Rust core and QML run on Android (arm64-v8a, Android 9+) with a phone layout: bottom navigation,
a mini player above it and a full-screen player, menus as bottom sheets, touch instead of hover. Playback keeps
going in the background through a media notification (lock screen, headset buttons, audio focus). Sign-in works
the same way: the browser opens and Spotify sends it back to `127.0.0.1`. Tokens are stored as files in the app's
private directory (Android has no keyring for native code); backups of the app are turned off.

You need Rust with the `aarch64-linux-android` target, JDK 17+, the Android SDK (platform 36, build-tools 36) with
NDK r28, and two Qt 6.12 installs: for Android and for the host (its tools build the package).

```bash
rustup target add aarch64-linux-android
python3 .github/scripts/install_qt.py --host android --out ~/Qt/6.12.0/android_arm64_v8a
python3 .github/scripts/install_qt.py --out ~/Qt/6.12.0/gcc_64
export QT_ANDROID=~/Qt/6.12.0/android_arm64_v8a QT_HOST=~/Qt/6.12.0/gcc_64
export ANDROID_SDK_ROOT=~/Android/Sdk ANDROID_NDK_ROOT=~/Android/Sdk/ndk/28.2.13676358
android/build_apk.sh --release   # target/android/Materialfy-<version>-arm64-v8a-debugsigned.apk
```

The player can also sit in the system "island": on Android 16 as a Live Update (status bar chip, Samsung
Now Bar, HyperOS Super Island, ColorOS Fluid Cloud) and on vivo phones in the OriginOS Origin Island
(Settings → Playback → "Player in the system island"). vivo shows the island's music card only for apps the
firmware allows, so on some firmware only the regular player notification appears. Discord shows "Listening to
Spotify" through the Spotify connection in Discord itself: Materialfy plays as a Spotify Connect device, so it
works the same on Android.

Updates come from this fork's GitHub Releases (`makdren/materialfy`): Settings → Updates downloads the
`-arm64-v8a.apk` of the newest release, checks it against `SHA256SUMS.txt` and hands it to the system installer.
Android installs an update only over an APK signed with the same key, so the release build in CI
(`build.yml`, job `android`) signs with the `ANDROID_KEYSTORE_BASE64` and `ANDROID_KEYSTORE_PASS` repository
secrets (key alias `materialfy`, or the `ANDROID_KEY_ALIAS` variable). A `v*` tag publishes a release with the APK;
without the key no release is created.

Without a keystore the APK is signed with the SDK debug key: it installs as is, but a build signed with a
different key cannot be installed over it. For a release key set `ANDROID_KEYSTORE`, `ANDROID_KEY_ALIAS` and
`ANDROID_KEYSTORE_PASS`. The Android part lives in [`android/`](android) (manifest, activity, playback and sign-in
services) and [`crates/materialfy/cpp/mobile.cpp`](crates/materialfy/cpp/mobile.cpp); the phone layout is
[`MobileMain.qml`](src/materialfy/qml/MobileMain.qml). On a desktop `--mobile` opens the same layout in a window,
for example `--mobile --size 412x915`.

### Command-line flags and variables

| Flag | What it does |
|---|---|
| `--offline` | do not restore the sign-in from the secret store |
| `--new-instance` | start a second instance even if the app is already open |
| `--light` | light theme |
| `--size 1280x800` | window size |
| `--screenshot shot.png`, `--delay 1500` | save a screenshot of the window after the given number of milliseconds and exit |
| `--config-dir DIR` | separate settings directory |
| `--set key=value` | initial window property, for example `--set startPage=settings` |
| `--gallery` | Material 3 component gallery |
| `--mobile` | phone layout (as on Android) |

Environment variables: `MATERIALFY_QML_DIR` sets a custom QML directory, `MATERIALFY_SESSION_FILE` sets the
player session file, `RUST_LOG` controls logging (for example `RUST_LOG=materialfy=debug,sf_player=debug`).

### Tests and CI

```bash
cargo test --locked --workspace
```

[`build.yml`](.github/workflows/build.yml) runs on pushes and pull requests to `main` and manually. In this fork it
builds only the APK; the Windows, Linux and macOS jobs are skipped unless the repository variable `DESKTOP_BUILDS`
is `true`.

## How it works

The interface is written in QML; the Material 3 Expressive components live in `src/materialfy/qml/Md3`. The Rust
core is connected to QML through [cxx-qt](https://github.com/KDAB/cxx-qt). Async code runs in a single tokio
runtime, and the Qt UI thread receives ready-made state snapshots.

| Crate | What it does |
|---|---|
| `md3-model` | data types for QML: tracks, albums, playlists, devices |
| `sf-auth` | OAuth PKCE sign-in, storing permissions in the system secret store, proxy |
| `sf-api` | Spotify Web API client: priority queues, GET merging and caching, cooldown after 429 and quota errors |
| `sf-player` | librespot and Spotify Connect, queue, reconciling local and remote playback |
| `sf-services` | sign-in, library, catalog, radio, home, lyrics, devices, history |
| `mf-theme` | Material You schemes from a cover or a chosen color |
| `mf-lyrics` | lyrics from several sources, LRC parsing, cache |
| `mf-platform` | directories, settings, MPRIS, single instance |
| `mf-i18n` | interface translation: English strings in the code, Russian and Portuguese dictionaries from `i18n/ru/*.json` and `i18n/pt/*.json` |
| `materialfy` | the executable: QML bridges, command-line flags, image cache, window frame on Windows |

## Origin and license

This repository is a fork of [fatyzzz/materialfy](https://github.com/fatyzzz/materialfy) that adds the Android
version (`android/`, `MobileMain.qml`, `crates/materialfy/cpp/mobile.cpp`). The license is the same.

Materialfy is based on two MIT-licensed projects:

- [spotifast](https://github.com/crmne/spotifast) (Carmine Paolino, crmne): sign-in, Web API client, playback
  through librespot, Spotify Connect, queue, library and radio (`crates/sf-*`).
- [yamusic-md3](https://github.com/yaskorinov/yamusic-md3) (yaskorinov and contributors): the QML interface and
  the Material 3 Expressive component library, the cxx-qt shell, cover-based theming, lyrics, platform services,
  build scripts (`src/materialfy/qml`, `crates/materialfy`, `crates/mf-*`).

Third-party components: [librespot](https://github.com/librespot-org/librespot) (MIT, our own fork
[fatyzzz/librespot](https://github.com/fatyzzz/librespot) with crmne's patches),
[fastframe](https://github.com/dennisgr7/fastframe) (MIT, audio output and media keys),
[cxx-qt](https://github.com/KDAB/cxx-qt) (MIT or Apache-2.0), [Qt 6](https://www.qt.io) (LGPL-3.0, linked
dynamically), the Google Sans font (SIL Open Font License 1.1) and Material Symbols Rounded icons (Apache License
2.0).

Materialfy is licensed under the GNU General Public License, version 3 or any later version
(GPL-3.0-or-later); the text is in [LICENSE](LICENSE). Releases up to and including 0.2.1 were published under the
MIT License, and the code taken from spotifast and yamusic-md3 keeps its MIT notice in
[LICENSES/MIT.txt](LICENSES/MIT.txt). Authors, sources and dependency
licenses are listed in [NOTICE](NOTICE). The "Wave-M" logo is in [`docs/brand/`](docs/brand/).

<br>

<p align="center"><sub>Materialfy is not affiliated with or endorsed by Spotify AB. Spotify is a trademark of Spotify AB.</sub></p>
