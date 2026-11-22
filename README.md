# Vinyl

A local music player for Windows: your own songs, recommendations, new music from similar artists, and a record that spins on the Now Playing page. Rust server (`tiny_http`) in a `wry` desktop window, built on [music-rs](../music-rs).

## Install

Download **vinyl-setup.exe** from the [latest release](https://github.com/HunterGHall/vinyl/releases/latest) and run it. It installs for your user (no admin prompt) and can set up `yt-dlp` and `ffmpeg` with winget, which Vinyl uses to add music. It needs the Microsoft Edge WebView2 runtime, which comes with Windows 11.

Your songs, playlists and settings live in `%LOCALAPPDATA%\Vinyl`, so updates and uninstalling never touch them.

**Updates:** each time Vinyl starts it checks the latest GitHub release. If there is a newer version it asks whether to install it; saying yes downloads `vinyl-setup.exe`, verifies its SHA-256 against GitHub's, runs it, and Vinyl reopens updated. Settings has a "Check for updates" button.

<<<<<<< HEAD
## Releasing a new version

1. Push your changes.
2. Create a release on GitHub with a new version tag, e.g. `v0.0.2` (or just push the tag).

GitHub Actions (`.github/workflows/release.yml`) then builds `vinyl-setup.exe` and attaches it to that release, which is what installed copies look for when they check for updates. The version comes from the tag (`v0.0.2` builds version `0.0.2`), so there is nothing to bump in `Cargo.toml`; the build stops with a clear error if the tag isn't like `v1.2.3`. You can also run the workflow by hand for an existing tag from the Actions tab.

To build locally instead: `.\installeruild.ps1` makes `distinyl-setup.exe` (needs Inno Setup 6: `winget install JRSoftware.InnoSetup`), and `.\installer
elease.ps1` also publishes it with the `gh` CLI.

## Running from source

```powershell
cargo run --release           # desktop window
cargo run --release -- --web  # just serve http://127.0.0.1:8735 (VINYL_PORT to change)
```

From a checkout, if the current folder already has `library\` or `data\`, Vinyl uses it; otherwise it uses `%LOCALAPPDATA%\Vinyl`. Set `MUSIC_HOME` to choose a folder. `MUSIC_DEV_STATIC=1` serves `static\` from disk. `VINYL_MEDIA_KEYS=1` also enables the media keys in `--web` mode (for testing). `VINYL_RELEASES_URL` points the updater at another releases endpoint (used for testing).

=======
>>>>>>> 8f77cbff8615b9462e6cd2e566ff51dca127dc9c
## What's in it

- **Design**: sidebar with search, a floating player dock with a little spinning record, rounded covers, one orange accent.
- **Now Playing**: the record is rendered as live ASCII (ported from `../ascii-vinyl`, with its panel settings fixed: rotate on, 1.10 rad/s, clockwise, light 163 degrees, 1.00x, cover 300 px / radius 120 px, 5 px cells, classic characters, original colours), on a platter tinted by the cover, next to time-synced lyrics.
- **Favorites**, **Daily Mixes**, **Discover** (Deezer related artists and genres), starting from nothing with genre picks, playlists, and **Settings** for resetting your data.
- **Albums**: album names come from catalog downloads and are looked up in the background for everything else (iTunes). The Albums page groups your songs, songs show their album, and search finds albums.
- **Media keys**: play/pause, next, previous and stop work system-wide, even when Vinyl is in the background (the desktop app registers them; if another app already owns them, the browser's media session is used).
- Songs from Discover download first, then play from your library.

Shortcuts: `Space` play/pause, `Ctrl+←/→` previous/next, `←/→` seek 5 s, `L` like, `S` shuffle, `/` search, `Esc` closes Now Playing.
