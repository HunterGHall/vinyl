//! The library: tracks on disk plus the JSON records that describe them
//! (metadata, play counts, loudness, download sources, settings, playlists).

use crate::store::{self, lock, paths, read_arr, read_obj, save};
use crate::util::{self, quote, words};
use regex_lite::Regex;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

pub const AUDIO_EXTS: [&str; 7] = ["mp3", "wav", "m4a", "flac", "ogg", "opus", "aac"];
pub const TRANSITION_MODES: [&str; 3] = ["off", "gapless", "crossfade"];
const TARGET_LUFS: f64 = -14.0; // what every track is levelled to (streaming-service norm)

// ---------------- file helpers ----------------

pub fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

pub fn file_stem(path: &Path) -> String {
    path.file_stem().unwrap_or_default().to_string_lossy().into_owned()
}

pub fn mtime(path: &Path) -> Option<f64> {
    let t = fs::metadata(path).ok()?.modified().ok()?;
    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs_f64())
}

pub fn find_track_paths() -> Vec<PathBuf> {
    let p = paths();
    let _ = fs::create_dir_all(&p.library);
    let mut out: Vec<PathBuf> = fs::read_dir(&p.library)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|path| {
                    path.is_file()
                        && path
                            .extension()
                            .map(|x| AUDIO_EXTS.contains(&x.to_string_lossy().to_lowercase().as_str()))
                            .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|p| file_name(p).to_lowercase());
    out
}

/// The library file for a track id, or None if the id isn't a plain
/// filename inside the library (ids arrive from the client).
pub fn safe_track_path(track_id: &str) -> Option<PathBuf> {
    if track_id.is_empty() {
        return None;
    }
    let path = paths().library.join(track_id);
    let parent = fs::canonicalize(path.parent()?).ok()?;
    let lib = fs::canonicalize(&paths().library).ok()?;
    (parent == lib).then_some(path)
}

pub fn cover_path_for(audio: &Path) -> PathBuf {
    paths().covers.join(format!("{}.jpg", file_stem(audio)))
}

pub fn cover_url(audio: &Path) -> Value {
    let cover = cover_path_for(audio);
    if cover.is_file() {
        json!(format!("/covers/{}", quote(&file_name(&cover))))
    } else {
        Value::Null
    }
}

// ---------------- durations ----------------

fn ffprobe_duration(path: &Path) -> Option<f64> {
    let out = util::command("ffprobe")
        .args(["-v", "quiet", "-show_entries", "format=duration", "-of", "default=nw=1:nk=1"])
        .arg(path)
        .output();
    match out {
        Ok(o) => {
            let v = String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok();
            if v.is_none() {
                store::warn(format!("ffprobe couldn't read the length of {}", file_name(path)));
            }
            v
        }
        Err(e) => {
            store::warn(format!("ffprobe couldn't read the length of {}: {e}", file_name(path)));
            None
        }
    }
}

// Song lengths are remembered in data/durations.json (name -> [mtime, seconds])
// so a launch doesn't re-run ffprobe (~0.15 s each) over the whole library.
#[derive(Default)]
struct Durations {
    map: HashMap<String, (f64, f64)>,
    loaded: bool,
    dirty: bool,
}

static DURATIONS: LazyLock<Mutex<Durations>> = LazyLock::new(Default::default);

fn with_durations<R>(f: impl FnOnce(&mut Durations) -> R) -> R {
    let mut d = DURATIONS.lock().unwrap_or_else(|e| e.into_inner());
    if !d.loaded {
        d.loaded = true;
        if let Value::Object(o) = read_obj(&paths().data.join("durations.json")) {
            for (k, v) in o {
                if let (Some(m), Some(s)) = (v[0].as_f64(), v[1].as_f64()) {
                    d.map.insert(k, (m, s));
                }
            }
        }
    }
    f(&mut d)
}

fn cached_duration(name: &str, m: f64) -> Option<f64> {
    with_durations(|d| d.map.get(name).filter(|(mt, _)| (mt - m).abs() < 1e-3).map(|(_, s)| *s))
}

/// Length in seconds, from the cache when the file is unchanged.
pub fn track_duration(path: &Path) -> f64 {
    let Some(m) = mtime(path) else { return 0.0 };
    let name = file_name(path);
    if let Some(d) = cached_duration(&name, m) {
        return d;
    }
    let Some(d) = ffprobe_duration(path) else { return 0.0 }; // failures aren't remembered
    with_durations(|c| {
        c.map.insert(name, (m, d));
        c.dirty = true;
    });
    d
}

/// Measures every uncached song up front, several at a time.
pub fn warm_durations(paths_: &[PathBuf]) {
    let missing: Vec<&PathBuf> = paths_
        .iter()
        .filter(|p| mtime(p).is_some_and(|m| cached_duration(&file_name(p), m).is_none()))
        .collect();
    if missing.len() < 2 {
        return;
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..missing.len().min(8) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(p) = missing.get(i) else { break };
                    track_duration(p);
                }
            });
        }
    });
    flush_durations();
}

pub fn flush_durations() {
    let snapshot = with_durations(|d| {
        if !d.dirty {
            return None;
        }
        d.dirty = false;
        let o: Map<String, Value> = d.map.iter().map(|(k, (m, s))| (k.clone(), json!([m, s]))).collect();
        Some(Value::Object(o))
    });
    if let Some(v) = snapshot {
        let _ = store::write_json(&paths().data.join("durations.json"), &v, false, false);
    }
}

/// Saves newly measured lengths shortly after they appear.
pub fn start_background_tasks() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(8));
        backfill_albums_async();
    });
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3));
            flush_durations();
        }
    });
}

// ---------------- metadata (user-edited names) ----------------

pub fn load_meta() -> Value {
    read_obj(&paths().meta)
}
pub fn save_meta(meta: &Value) {
    save(&paths().meta, meta);
}

/// (title, artist) - by default the artist comes from an "Artist - Title.ext"
/// filename; a title/artist stored in the metadata file takes precedence.
pub fn name_fields(path: &Path, meta: &Value) -> (String, String) {
    let name = file_stem(path).replace('⧸', "/");
    let (mut artist, mut title) = match name.split_once(" - ") {
        Some((a, t)) => (a.to_string(), t.to_string()),
        None => (String::new(), name),
    };
    let ov = &meta[file_name(path)];
    if ov.as_object().is_some_and(|o| !o.is_empty()) {
        if let Some(t) = ov["title"].as_str() {
            title = t.to_string();
        }
        if let Some(a) = ov["artist"].as_str() {
            artist = a.to_string();
        }
    }
    (title, artist)
}

/// Filenames can't hold / \ : * ? " < > | so they're swapped for "_" on disk,
/// which would show "AC/DC" as "AC_DC". When the real title/artist differ from
/// what the filename gives back, keep them in the metadata file (flagged "auto"
/// so the song doesn't count as user-edited).
pub fn remember_real_names(final_path: &Path, title: &str, artist: &str) {
    let (title, artist) = (title.trim(), artist.trim());
    if title.is_empty() {
        return;
    }
    let on_disk = name_fields(final_path, &Value::Null);
    if (title, artist) == (on_disk.0.as_str(), on_disk.1.as_str()) {
        return;
    }
    // Only act when the names differ in punctuation, never when the filename
    // genuinely carries different words.
    if words(title) != words(&on_disk.0) || words(artist) != words(&on_disk.1) {
        return;
    }
    let _g = lock();
    let mut meta = load_meta();
    meta[file_name(final_path)] = json!({"title": title, "artist": artist, "auto": true});
    save_meta(&meta);
}

// ---------------- stats ----------------

pub fn load_stats() -> Value {
    read_obj(&paths().stats)
}
pub fn save_stats(stats: &Value) {
    save(&paths().stats, stats);
}

pub fn record_play(track_id: &str) -> i64 {
    let _g = lock();
    let mut stats = load_stats();
    let n = stats[track_id].as_i64().unwrap_or(0) + 1;
    stats[track_id] = json!(n);
    save_stats(&stats);
    // Timestamped listening history (newest last, capped) feeds "recently
    // played" and the recommendations.
    let mut history = read_arr(&paths().history);
    if let Some(h) = history.as_array_mut() {
        h.push(json!({"id": track_id, "at": store::now()}));
        if h.len() > HISTORY_CAP {
            h.drain(..h.len() - HISTORY_CAP);
        }
    }
    save(&paths().history, &history);
    n
}

const HISTORY_CAP: usize = 2000;

pub fn load_history() -> Value {
    read_arr(&paths().history)
}

// ---------------- likes ----------------
// data/likes.json: [{"id": <track id>, "at": <unix time>}], oldest first.

pub fn load_likes() -> Value {
    read_arr(&paths().likes)
}

/// Likes or unlikes a track; returns whether it is liked afterwards.
pub fn set_liked(track_id: &str, liked: bool) -> bool {
    let _g = lock();
    let mut likes = load_likes();
    let list = likes.as_array_mut().unwrap();
    let present = list.iter().any(|l| l["id"] == track_id);
    if liked && !present {
        list.push(json!({"id": track_id, "at": store::now()}));
        save(&paths().likes, &likes);
    } else if !liked && present {
        list.retain(|l| l["id"] != track_id);
        save(&paths().likes, &likes);
    }
    liked
}

// ---------------- loudness ----------------

static LOUDNESS_LOCK: Mutex<()> = Mutex::new(()); // one ffmpeg measurement at a time
static SCAN_RUNNING: AtomicBool = AtomicBool::new(false);
static LUFS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"I:\s+(-?\d+(?:\.\d+)?)\s+LUFS").unwrap());
static PEAK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Peak:\s+(-?\d+(?:\.\d+)?)\s+dBFS").unwrap());

pub fn load_loudness() -> Value {
    read_obj(&paths().loudness)
}

/// (integrated LUFS, peak dBFS), or None if it can't be measured.
fn measure_loudness(path: &Path) -> Option<(f64, f64)> {
    let out = util::command("ffmpeg")
        .args(["-nostats", "-hide_banner", "-i"])
        .arg(path)
        .args(["-af", "ebur128=peak=true:framelog=quiet", "-f", "null", "-"])
        .output();
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stderr).into_owned();
            let summary = text.rsplit("Summary:").next().unwrap_or("");
            let lufs: f64 = LUFS_RE.captures_iter(summary).last()?[1].parse().ok()?;
            let peak: f64 = PEAK_RE.captures_iter(summary).last()?[1].parse().ok()?;
            Some((lufs, peak))
        }
        Err(e) => {
            store::warn(format!("Loudness measurement failed for {}: {e}", file_name(path)));
            None
        }
    }
}

/// Gain to reach the target loudness, capped so boosting never pushes the peak
/// over full scale (no clipping) and so one odd file can't swing too far.
fn gain_db_from(lufs: f64, peak: f64) -> f64 {
    let gain = (TARGET_LUFS - lufs).min(-peak - 1.0).max(-15.0);
    (gain * 100.0).round() / 100.0
}

pub fn cached_gain_db(path: &Path, loudness: &Value) -> Option<f64> {
    let entry = &loudness[file_name(path)];
    let (m, lufs, peak) = (entry["mtime"].as_f64()?, entry["lufs"].as_f64()?, entry["peak"].as_f64()?);
    ((m - mtime(path)?).abs() < 1e-3).then(|| gain_db_from(lufs, peak))
}

/// Measures (if not cached) and returns the gain in dB, or None.
pub fn ensure_loudness(path: &Path) -> Option<f64> {
    let _g = LOUDNESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut loudness = load_loudness();
    if let Some(g) = cached_gain_db(path, &loudness) {
        return Some(g);
    }
    let (lufs, peak) = measure_loudness(path)?;
    loudness[file_name(path)] = json!({"mtime": mtime(path)?, "lufs": lufs, "peak": peak});
    save(&paths().loudness, &loudness);
    Some(gain_db_from(lufs, peak))
}

/// Measures any tracks not yet cached, in the background.
pub fn scan_loudness_async() {
    if SCAN_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        for path in find_track_paths() {
            ensure_loudness(&path);
        }
        SCAN_RUNNING.store(false, Ordering::SeqCst);
    });
}

// ---------------- track -> JSON ----------------

pub fn track_to_dict(path: &Path, stats: &Value, loudness: &Value, meta: &Value) -> Value {
    let (title, artist) = name_fields(path, meta);
    let name = file_name(path);
    let edited = meta[&name].is_object() && !meta[&name]["auto"].as_bool().unwrap_or(false);
    json!({
        "edited": edited,
        "gain_db": cached_gain_db(path, loudness),
        "id": name,
        "title": title,
        "artist": artist,
        "duration": track_duration(path),
        "cover": cover_url(path),
        "album": meta[&name]["album"].as_str().unwrap_or(""),
        "plays": stats[&name].as_i64().unwrap_or(0),
        "added": mtime(path).unwrap_or(0.0),
    })
}

// ---------------- download sources (duplicate detection) ----------------
// Maps a stable source key ("yt:<video id>" / "spotify:<track id>") to the
// library filename it was downloaded as, so re-adding the same link is a no-op.

pub fn load_sources() -> Value {
    read_obj(&paths().sources)
}

static SPOTIFY_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"open\.spotify\.com/(?:intl-[a-z]+/)?track/([A-Za-z0-9]+)|spotify:track:([A-Za-z0-9]+)").unwrap()
});
static YT_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:[?&]v=|youtu\.be/|/shorts/|/embed/)([A-Za-z0-9_-]{11})").unwrap());

pub fn source_key(url: &str) -> Option<String> {
    if let Some(c) = SPOTIFY_KEY_RE.captures(url) {
        let id = c.get(1).or_else(|| c.get(2))?.as_str();
        return Some(format!("spotify:{id}"));
    }
    if url.contains("youtube.com") || url.contains("youtu.be") {
        if let Some(c) = YT_KEY_RE.captures(url) {
            return Some(format!("yt:{}", &c[1]));
        }
    }
    None
}

/// Path of the library file already downloaded from this source, if it still
/// exists (a deleted track is no longer a duplicate).
pub fn existing_for_source(key: &str) -> Option<PathBuf> {
    let name = {
        let _g = lock();
        load_sources()[key].as_str().map(String::from)?
    };
    let path = paths().library.join(name);
    path.is_file().then_some(path)
}

pub fn record_source(key: &str, filename: &str) {
    let _g = lock();
    let mut sources = load_sources();
    sources[key] = json!(filename);
    save(&paths().sources, &sources);
}

// ---------------- settings ----------------

fn default_settings() -> Map<String, Value> {
    let Value::Object(m) = json!({
        "accent_color": "#ffffff",
        "normalize_volume": true,
        "transition_mode": "gapless",   // off | gapless | crossfade
        "crossfade_seconds": 5,
        "lyrics_enabled": true,
        "spotify_sync_url": "",         // Spotify playlist watched for new songs ("" = off)
    }) else { unreachable!() };
    m
}

pub fn load_settings() -> Value {
    let mut settings = default_settings();
    if let Value::Object(data) = read_obj(&paths().settings) {
        for (k, v) in data {
            if settings.contains_key(&k) {
                settings.insert(k, v);
            }
        }
    }
    Value::Object(settings)
}

pub fn save_settings(settings: &Value) {
    save(&paths().settings, settings);
}

pub fn lyrics_enabled() -> bool {
    load_settings()["lyrics_enabled"].as_bool().unwrap_or(true)
}

// ---------------- playlists ----------------

pub fn load_playlists() -> Value {
    read_arr(&paths().playlists)
}
pub fn save_playlists(playlists: &Value) {
    save(&paths().playlists, playlists);
}

pub fn find_playlist<'a>(playlists: &'a mut Value, pid: &str) -> Option<&'a mut Value> {
    playlists.as_array_mut()?.iter_mut().find(|p| p["id"] == pid)
}

/// The `track_ids` of a playlist as a Vec<String>.
pub fn track_ids_of(pl: &Value) -> Vec<String> {
    pl["track_ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn create_playlist_record(name: &str) -> Value {
    let _g = lock();
    let mut playlists = load_playlists();
    let playlist = json!({"id": new_id(), "name": name, "track_ids": []});
    playlists.as_array_mut().unwrap().push(playlist.clone());
    save_playlists(&playlists);
    playlist
}

pub fn rename_playlist_record(pid: &str, name: &str) {
    let _g = lock();
    let mut playlists = load_playlists();
    if let Some(pl) = find_playlist(&mut playlists, pid) {
        pl["name"] = json!(name);
        save_playlists(&playlists);
    }
}

pub fn add_track_to_playlist_record(pid: &str, track_id: &str) {
    let _g = lock();
    let mut playlists = load_playlists();
    if let Some(pl) = find_playlist(&mut playlists, pid) {
        if !track_ids_of(pl).iter().any(|t| t == track_id) {
            pl["track_ids"].as_array_mut().unwrap().push(json!(track_id));
            save_playlists(&playlists);
        }
    }
}

// ---------------- deleting ----------------

fn remove_if_file(path: &Path) -> bool {
    path.is_file() && fs::remove_file(path).is_ok()
}

/// Deletes song files (and covers) and scrubs them from playlists, play counts
/// and saved edits. Returns how many files were removed.
pub fn delete_tracks(track_ids: &[String]) -> usize {
    let mut deleted = 0;
    let mut ids: Vec<&String> = Vec::new();
    for id in track_ids {
        let Some(path) = safe_track_path(id) else { continue };
        ids.push(id);
        if remove_if_file(&path) {
            deleted += 1;
        }
        remove_if_file(&cover_path_for(&path));
    }
    if ids.is_empty() {
        return 0;
    }
    let gone = |t: &str| ids.iter().any(|i| i.as_str() == t);
    {
        let _g = lock();
        let mut playlists = load_playlists();
        for pl in playlists.as_array_mut().unwrap() {
            let kept: Vec<Value> = track_ids_of(pl).into_iter().filter(|t| !gone(t)).map(Value::from).collect();
            pl["track_ids"] = Value::Array(kept);
        }
        save_playlists(&playlists);
        for (load, save_fn) in [
            (load_stats as fn() -> Value, save_stats as fn(&Value)),
            (load_meta, save_meta),
        ] {
            let mut data = load();
            if let Some(o) = data.as_object_mut() {
                let before = o.len();
                o.retain(|k, _| !gone(k));
                if o.len() != before {
                    save_fn(&data);
                }
            }
        }
    }
    {
        let _g = lock();
        for (path, key) in [(paths().likes.clone(), "id"), (paths().history.clone(), "id")] {
            let mut list = read_arr(&path);
            if let Some(a) = list.as_array_mut() {
                let before = a.len();
                a.retain(|e| !e[key].as_str().is_some_and(gone));
                if a.len() != before {
                    save(&path, &list);
                }
            }
        }
    }
    for id in &ids {
        let _ = fs::remove_file(crate::lyrics::cache_path(id));
    }
    let _g = LOUDNESS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut loudness = load_loudness();
    if let Some(o) = loudness.as_object_mut() {
        let before = o.len();
        o.retain(|k, _| !gone(k));
        if o.len() != before {
            save(&paths().loudness, &loudness);
        }
    }
    deleted
}

// ---------------- albums ----------------
// Album names live in the metadata file next to title/artist. Catalog downloads know theirs;
// everything else is looked up once on the iTunes catalog in the background ("album_checked"
// stops a song that has no match being looked up again).

pub fn remember_album(path: &Path, album: &str) {
    let album = album.trim();
    if album.is_empty() {
        return;
    }
    let _g = lock();
    let mut meta = load_meta();
    let name = file_name(path);
    if !meta[&name].is_object() {
        meta[&name] = json!({"auto": true});
    }
    meta[&name]["album"] = json!(album);
    meta[&name]["album_checked"] = json!(true);
    save_meta(&meta);
}

fn mark_album_checked(name: &str) {
    let _g = lock();
    let mut meta = load_meta();
    if !meta[name].is_object() {
        meta[name] = json!({"auto": true});
    }
    meta[name]["album_checked"] = json!(true);
    save_meta(&meta);
}

/// "Boston - Single" / "Boston - EP" -> "Boston" (a single's own name is a fine album name).
fn clean_album(name: &str) -> String {
    let n = name.trim();
    for suffix in [" - Single", " - EP"] {
        if let Some(base) = n.strip_suffix(suffix) {
            return base.trim().to_string();
        }
    }
    n.to_string()
}

fn lookup_album(title: &str, artist: &str, duration: f64) -> Option<String> {
    let term = format!("{artist} {title}");
    let url = format!("https://itunes.apple.com/search?media=music&entity=song&limit=8&term={}", util::quote_all(term.trim()));
    let text = util::http_get_text(&url, 12, 2).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let (want_title, want_artist) = (words(&crate::lyrics::clean_title(title)), words(&crate::lyrics::first_artist(artist)));
    let mut best: Option<(f64, String)> = None;
    for r in v["results"].as_array()? {
        let (Some(t), Some(a), Some(album)) = (r["trackName"].as_str(), r["artistName"].as_str(), r["collectionName"].as_str()) else { continue };
        if words(&crate::lyrics::clean_title(t)) != want_title {
            continue;
        }
        let ra = words(a);
        if !want_artist.is_empty() && !ra.contains(&want_artist) && !want_artist.contains(&ra) {
            continue;
        }
        let diff = if duration > 0.0 { (r["trackTimeMillis"].as_f64().unwrap_or(0.0) / 1000.0 - duration).abs() } else { 0.0 };
        if diff > 15.0 {
            continue;
        }
        if best.as_ref().is_none_or(|(d, _)| diff < *d) {
            best = Some((diff, clean_album(album)));
        }
    }
    best.map(|(_, a)| a).filter(|a| !a.is_empty())
}

static ALBUMS_RUNNING: AtomicBool = AtomicBool::new(false);

/// Looks up the album of every song that doesn't have one yet, politely, one at a time.
pub fn backfill_albums_async() {
    if ALBUMS_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        // Songs added while this runs are picked up by going round again until nothing is left.
        loop {
            let todo: Vec<(PathBuf, String, String)> = {
                let meta = load_meta();
                find_track_paths()
                    .into_iter()
                    .filter(|p| !meta[file_name(p)]["album_checked"].as_bool().unwrap_or(false) && meta[file_name(p)]["album"].as_str().is_none_or(str::is_empty))
                    .map(|p| {
                        let (t, a) = name_fields(&p, &meta);
                        (p, t, a)
                    })
                    .collect()
            };
            if todo.is_empty() {
                break;
            }
            for (path, title, artist) in todo {
                if !path.is_file() {
                    mark_album_checked(&file_name(&path));
                    continue;
                }
                match lookup_album(&title, &artist, track_duration(&path)) {
                    Some(album) => remember_album(&path, &album),
                    None => mark_album_checked(&file_name(&path)),
                }
                std::thread::sleep(std::time::Duration::from_millis(450));
            }
        }
        ALBUMS_RUNNING.store(false, Ordering::SeqCst);
    });
}
