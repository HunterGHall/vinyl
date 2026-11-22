# Vinyl

A local music player for Windows: your own songs, recommendations, new music from similar artists, and a record that spins on the Now Playing page. Rust server (`tiny_http`) in a `wry` desktop window, built on [music-rs](../music-rs).

## Install

Download **vinyl-setup.exe** from the [latest release](https://github.com/HunterGHall/vinyl/releases/latest) and run it. It installs for your user (no admin prompt) and can set up `yt-dlp` and `ffmpeg` with winget, which Vinyl uses to add music. It needs the Microsoft Edge WebView2 runtime, which comes with Windows 11.

Your songs, playlists and settings live in `%LOCALAPPDATA%\Vinyl`, so updates and uninstalling never touch them.

**Updates:** each time Vinyl starts it checks the latest GitHub release. If there is a newer version it asks whether to install it; saying yes downloads `vinyl-setup.exe`, verifies its SHA-256 against GitHub's, runs it, and Vinyl reopens updated. Settings has a "Check for updates" button.

## What's in it

- **Design**: sidebar with search, a floating player dock with a little spinning record, rounded covers, one orange accent.
- **Now Playing**: the record is rendered as live ASCII (ported from `../ascii-vinyl`, with its panel settings fixed: rotate on, 1.10 rad/s, clockwise, light 163 degrees, 1.00x, cover 300 px / radius 120 px, 5 px cells, classic characters, original colours), on a platter tinted by the cover, next to time-synced lyrics.
- **Favorites**, **Daily Mixes**, **Discover** (Deezer related artists and genres), starting from nothing with genre picks, playlists, and **Settings** for resetting your data.
- Songs from Discover download first, then play from your library.

Shortcuts: `Space` play/pause, `Ctrl+←/→` previous/next, `←/→` seek 5 s, `L` like, `S` shuffle, `/` search, `Esc` closes Now Playing.
