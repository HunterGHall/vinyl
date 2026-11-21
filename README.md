# Vinyl

A Spotify-style local music player, built on [music-rs](../music-rs): same Rust server (`tiny_http`), same `wry` desktop window, same vinyl-record logo, new front end.

```powershell
cargo run --release           # desktop window (needs yt-dlp, ffmpeg, ffprobe on PATH to add music)
cargo run --release -- --web  # just serve http://127.0.0.1:8735 (VINYL_PORT to change)
```

Data lives in `$MUSIC_HOME` (default: the current directory) as `library/`, `data/` and `static/`. It was copied from music-rs, so the library and playlists come along. Set `MUSIC_DEV_STATIC=1` to serve `static/` from disk instead of the copy packed into the exe.

## What's in it

- **Look**: a record-shop terminal. Black, cream and one orange, monospace labels over condensed type, square corners, a top tab bar, a "crates" list on the left and a deck along the bottom.
- **Now playing**: the NOW PLAYING button (or the deck's cover) opens a stage with the record rendered as live ASCII, ported from `../ascii-vinyl` with the settings from its panel (rotate on, 1.10 rad/s, clockwise, fixed light at 163 degrees, 1.00x, cover 300 px / radius 120 px, 5 px cells, classic characters, original colours). The disc turns while music plays and the cover sits on the label. The same settings panel is there, saved in the browser.
- **Favorites**: heart a song (or press `L`); they collect under Liked.
- **Recommendations from your library**: Daily Mixes, Because you listen to X, Discover, On repeat, Rediscover, song radio, autoplay of similar songs. They're built from plays, likes, history, playlists and artists added together.
- **New music** (Deezer public API, no key; `src/discover.rs`): related artists for the artists you listen to, so the new songs come from artists like yours, not just the same ones.
  - Each Daily Mix has new songs from related artists slipped in after every third song. They play instantly as 30 s previews, and playing a mix adds the first five to the library in the background and swaps the preview for the full song when it lands.
  - Home and Discover: "New music for you", "Artists like X". Artist pages: "Similar artists to discover".
  - Click NEW rows to preview, `+` to add.
- **Getting started with an empty library**: Home shows genre tiles. Pick a few, hit Build my library, and it downloads the top songs from each. PREVIEW on a tile (or the Discover tab) lets you hear a genre's chart first.

Downloads need yt-dlp and ffmpeg on PATH. Shortcuts: `Space` play/pause, `Ctrl+←/→` previous/next, `←/→` seek 5 s, `L` like, `S` shuffle, `/` search, `Esc` closes the stage.

Not carried over from music-rs: duplicate finder, name fixer, Spotify playlist watcher UI and the settings page (the backend routes remain).
