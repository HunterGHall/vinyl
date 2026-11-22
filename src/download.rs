//! Downloading: YouTube/Spotify links -> mp3 files in the library, via the
//! `yt-dlp` and `ffmpeg` command-line tools, tracked as background jobs.

use crate::library::{
    self, add_track_to_playlist_record, cover_path_for, create_playlist_record, existing_for_source, file_name,
    file_stem, record_source, remember_real_names, rename_playlist_record, source_key,
};
use crate::store::{self, paths};
use crate::util::{self, download_image, is_spotify_playlist_url, is_spotify_url, sanitize_filename};
use crate::{deps, lyrics, spotify};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex};

// ---------------- jobs ----------------

/// A background job's state, shared between its worker thread and the status route.
#[derive(Clone)]
pub struct Job(Arc<Mutex<Value>>);

impl Job {
    pub fn new(state: Value) -> Job {
        Job(Arc::new(Mutex::new(state)))
    }
    pub fn with<R>(&self, f: impl FnOnce(&mut Value) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    pub fn snapshot(&self) -> Value {
        self.with(|j| j.clone())
    }
    pub fn push(&self, key: &str, item: Value) {
        self.with(|j| j[key].as_array_mut().unwrap().push(item));
    }
}

pub static JOBS: LazyLock<Mutex<HashMap<String, Job>>> = LazyLock::new(Default::default);

pub fn register(job: &Job) -> String {
    let id = library::new_id();
    JOBS.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), job.clone());
    id
}

pub fn lookup(id: &str) -> Option<Job> {
    JOBS.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
}

fn new_download_job(kind: &str) -> Job {
    Job::new(json!({
        "kind": kind,                // "track" | "playlist"
        "status": "downloading",     // downloading | done | error
        "percent": "",               // label for the file being fetched: "42.0%" / "Converting..." / ...
        "fraction": 0.0,             // 0..1 progress of the file being fetched (numeric, for the bar)
        "current_title": null,
        "title": null,               // final title, for a "track" job
        "as_playlist": true,         // false when a playlist link was imported as plain songs
        "playlist_id": null,         // for a "playlist" job (null unless as_playlist)
        "playlist_name": null,
        "total": 0,
        "duplicate": false,          // a "track" job whose track was already in the library
        "skipped": [],               // [{id, title}, ...] already in library (playlist job)
        "added": [],                 // [{id, title}, ...]
        "failed": [],                // [{title, error}, ...]
        "error": null,
    }))
}

// ---------------- yt-dlp ----------------

struct Progress {
    status: String,
    downloaded: f64,
    total: f64,
    title: String,
}

fn num(s: &str) -> f64 {
    s.trim().parse().unwrap_or(0.0)
}

fn apply_progress(job: &Job, p: &Progress) {
    job.with(|j| {
        if p.status == "downloading" {
            if p.total > 0.0 {
                let f = (p.downloaded / p.total).clamp(0.0, 1.0);
                j["fraction"] = json!(f);
                j["percent"] = json!(format!("{:.1}%", f * 100.0));
            } else {
                j["percent"] = json!(""); // size unknown: the page shows an indeterminate bar
            }
            if !p.title.is_empty() && p.title != "NA" {
                j["current_title"] = json!(p.title);
            }
        } else if p.status == "finished" {
            j["fraction"] = json!(1.0);
            j["percent"] = json!("Converting...");
        }
    });
}

/// Runs yt-dlp on `target` (a URL or `ytsearch1:...`), downloading one video as
/// mp3 into the library. Returns its info dict.
fn run_ytdlp(target: &str, job: Option<&Job>) -> Result<Value, String> {
    let template = paths().library.join("%(title)s.%(ext)s");
    let mut child = util::command("yt-dlp")
        .args(["--no-warnings", "--newline", "--progress", "--no-simulate", "--no-playlist"])
        .args(["-f", "bestaudio/best", "-x", "--audio-format", "mp3", "--audio-quality", "192", "-o"])
        .arg(&template)
        .args(deps::find("ffmpeg").and_then(|p| p.parent().map(|d| ["--ffmpeg-location".to_string(), d.to_string_lossy().into_owned()])).unwrap_or_default())
        .args([
            "--progress-template",
            "download:PROG\t%(progress.status)s\t%(progress.downloaded_bytes)s\t%(progress.total_bytes)s\t%(progress.total_bytes_estimate)s\t%(info.title)s",
            "--print",
            "after_move:JSON:%()j",
        ])
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| deps::friendly(&format!("Couldn't run yt-dlp ({e})")))?;

    let stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        BufReader::new(stderr).lines().map_while(Result::ok).collect::<Vec<_>>()
    });

    let mut info: Option<Value> = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        if let Some(rest) = line.strip_prefix("JSON:") {
            if let Ok(v) = serde_json::from_str::<Value>(rest) {
                info = Some(v);
            }
        } else if let Some(rest) = line.strip_prefix("PROG\t") {
            let f: Vec<&str> = rest.splitn(5, '\t').collect();
            if let (Some(job), true) = (job, f.len() == 5) {
                let (dl, total, est) = (num(f[1]), num(f[2]), num(f[3]));
                apply_progress(
                    job,
                    &Progress {
                        status: f[0].to_string(),
                        downloaded: dl,
                        total: if total > 0.0 { total } else { est },
                        title: f[4].to_string(),
                    },
                );
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let errors = err_thread.join().unwrap_or_default();
    match info {
        Some(v) if status.success() => Ok(v),
        _ => Err(errors
            .iter()
            .rev()
            .find(|l| l.contains("ERROR"))
            .map(|l| l.trim_start_matches("ERROR: ").to_string())
            .or_else(|| errors.last().cloned())
            .map(|e| deps::friendly(&e))
            .unwrap_or_else(|| format!("yt-dlp failed ({status})"))),
    }
}

fn final_path(info: &Value) -> Result<PathBuf, String> {
    info["requested_downloads"]
        .as_array()
        .and_then(|a| a.last())
        .and_then(|d| d["filepath"].as_str())
        .or_else(|| info["filepath"].as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "yt-dlp didn't report where the file went".to_string())
}

fn str_field<'a>(info: &'a Value, key: &str) -> &'a str {
    info[key].as_str().unwrap_or("").trim()
}

/// Best-effort (title, artist) for a yt-dlp info dict. Prefers YouTube's own
/// "Music in this video" metadata (artist/track), which is far more reliable
/// than the raw video title, then falls back to the uploading channel's name.
pub fn youtube_track_fields(info: &Value) -> (String, String) {
    let first = |keys: &[&str]| keys.iter().map(|k| str_field(info, k)).find(|s| !s.is_empty()).unwrap_or("").to_string();
    let mut title = first(&["track", "title"]);
    let artist = first(&["artist", "creator", "channel", "uploader"]);
    if !artist.is_empty() {
        if let Some((head, rest)) = title.split_once(" - ") {
            if head.trim().to_lowercase() == artist.to_lowercase() {
                title = rest.trim().to_string();
            }
        }
    }
    (title, artist)
}

/// Renames a downloaded file to the "Artist - Title" convention that
/// `name_fields` parses artist/title from, moving its cover art along with it.
/// No-ops when no artist is known, the title already leads with it, or the
/// file is already gone. Returns (final_path, display_name).
fn rename_with_artist(path: &Path, title: &str, artist: &str) -> (PathBuf, String) {
    if artist.is_empty() || title.to_lowercase().starts_with(&artist.to_lowercase()) || !path.is_file() {
        return (path.to_path_buf(), title.to_string());
    }
    let display = format!("{artist} - {title}");
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let final_path = path.with_file_name(format!("{}{ext}", sanitize_filename(&display)));
    let same = std::fs::canonicalize(&final_path).ok().zip(std::fs::canonicalize(path).ok()).is_some_and(|(a, b)| a == b);
    if !same {
        let _ = std::fs::remove_file(&final_path);
        if std::fs::rename(path, &final_path).is_err() {
            return (path.to_path_buf(), title.to_string());
        }
        let old_cover = cover_path_for(path);
        if old_cover.is_file() {
            let new_cover = cover_path_for(&final_path);
            let _ = std::fs::remove_file(&new_cover);
            let _ = std::fs::rename(old_cover, new_cover);
        }
    }
    (final_path, display)
}

/// Downloads a single video, ignoring any ?list= playlist it's part of.
/// Returns (path, display title).
fn download_youtube(url: &str, job: Option<&Job>) -> Result<(PathBuf, String), String> {
    let info = run_ytdlp(url, job)?;
    let path = final_path(&info)?;
    let (mut title, artist) = youtube_track_fields(&info);
    if title.is_empty() {
        title = file_stem(&path);
    }
    let (final_path, display) = rename_with_artist(&path, &title, &artist);
    remember_real_names(&final_path, &title, &artist);
    if let Some(thumb) = info["thumbnail"].as_str() {
        download_image(thumb, &cover_path_for(&final_path));
    }
    if let Some(id) = info["id"].as_str() {
        record_source(&format!("yt:{id}"), &file_name(&final_path));
    }
    Ok((final_path, display))
}

/// Downloads a single Spotify track link by finding its best match on YouTube.
/// Returns (path, display_name, duplicate) - duplicate is true when the track
/// was already in the library and nothing was downloaded.
fn download_spotify(url: &str, job: Option<&Job>) -> Result<(PathBuf, String, bool), String> {
    let key = source_key(url);
    if let Some(existing) = key.as_deref().and_then(existing_for_source) {
        let stem = file_stem(&existing);
        return Ok((existing, stem, true));
    }
    let (title, artist, cover) = spotify::fetch_track_metadata(url)?;
    if title.is_empty() {
        return Err("Could not read that Spotify link - check it's a track URL.".into());
    }
    let display = if artist.is_empty() { title.clone() } else { format!("{artist} - {title}") };
    let expected = paths().library.join(format!("{}.mp3", sanitize_filename(&display)));
    if expected.is_file() {
        if let Some(k) = &key {
            record_source(k, &file_name(&expected));
        }
        let stem = file_stem(&expected);
        return Ok((expected, stem, true));
    }

    let query = if artist.is_empty() { format!("{title} audio") } else { format!("{artist} {title} audio") };
    let info = run_ytdlp(&format!("ytsearch1:{query}"), job).map_err(|e| {
        if e.contains("yt-dlp failed") { format!("No YouTube match found for \"{query}\"") } else { e }
    })?;
    let downloaded = final_path(&info)?;
    let final_path = paths().library.join(format!("{}.mp3", sanitize_filename(&display)));
    let same = std::fs::canonicalize(&final_path).ok().zip(std::fs::canonicalize(&downloaded).ok()).is_some_and(|(a, b)| a == b);
    if !same {
        let _ = std::fs::remove_file(&final_path);
        std::fs::rename(&downloaded, &final_path).map_err(|e| e.to_string())?;
    }
    remember_real_names(&final_path, &title, &artist);
    if let Some(c) = cover {
        download_image(&c, &cover_path_for(&final_path));
    }
    if let Some(k) = &key {
        record_source(k, &file_name(&final_path));
    }
    Ok((final_path, display, false))
}

/// (playlist title, [(video id, title)]) of a YouTube playlist, from a quick
/// listing that downloads nothing - so progress has a real total.
fn list_youtube_playlist(url: &str) -> (Option<String>, Vec<(String, String)>) {
    let out = util::command("yt-dlp")
        .args(["--flat-playlist", "--ignore-errors", "--no-warnings", "--print", "%(id)s\t%(playlist_title)s\t%(title)s"])
        .arg(url)
        .stdin(Stdio::null())
        .output();
    let Ok(out) = out else { return (None, vec![]) };
    let mut title = None;
    let mut entries = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut f = line.splitn(3, '\t');
        let (id, pl, t) = (f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or(""));
        if id.is_empty() || id == "NA" {
            continue;
        }
        if title.is_none() && !pl.is_empty() && pl != "NA" {
            title = Some(pl.to_string());
        }
        entries.push((id.to_string(), t.to_string()));
    }
    (title, entries)
}

/// Searches YouTube for songs matching `query` (nothing is downloaded).
/// Each result: id, url, title, artist (the channel), duration, thumbnail, in_library.
pub fn search_youtube(query: &str) -> Result<Vec<Value>, String> {
    let out = util::command("yt-dlp")
        .args(["--flat-playlist", "--no-warnings", "--ignore-errors", "--print"])
        .arg("%(id)s\t%(duration)s\t%(channel,uploader)s\t%(title)s")
        .arg(format!("ytsearch12:{query}"))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Couldn't run yt-dlp ({e}) - is it installed and on PATH?"))?;
    let mut results = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut f = line.splitn(4, '\t');
        let (id, duration, channel, title) =
            (f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or(""), f.next().unwrap_or(""));
        let duration: f64 = duration.parse().unwrap_or(0.0);
        // skip live streams / unknown lengths and hour-long mixes
        if id.len() != 11 || duration <= 0.0 || duration > 1500.0 || title.is_empty() {
            continue;
        }
        let artist = channel.trim_end_matches(" - Topic");
        results.push(json!({
            "id": id,
            "url": format!("https://www.youtube.com/watch?v={id}"),
            "title": title,
            "artist": if artist == "NA" { "" } else { artist },
            "duration": duration,
            "thumbnail": format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"),
            "in_library": existing_for_source(&format!("yt:{id}")).is_some(),
        }));
    }
    if results.is_empty() && !out.status.success() {
        return Err("Couldn't search right now. Check your connection and try again.".into());
    }
    Ok(results)
}

/// Searches Apple's public music catalog (no key needed) for songs matching
/// `query`. Used for clean artist/title/album art; the audio itself is matched
/// on YouTube when the song is downloaded.
pub fn search_catalog(query: &str) -> Result<Vec<Value>, String> {
    let url = format!(
        "https://itunes.apple.com/search?media=music&entity=song&limit=14&term={}",
        util::quote_all(query)
    );
    let text = util::http_get_text(&url, 15, 2)
        .map_err(|_| "Couldn't reach the music catalog. Check your connection and try again.".to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for r in v["results"].as_array().into_iter().flatten() {
        let (Some(id), Some(title)) = (r["trackId"].as_i64(), r["trackName"].as_str()) else { continue };
        let artist = r["artistName"].as_str().unwrap_or("");
        let duration = r["trackTimeMillis"].as_f64().unwrap_or(0.0) / 1000.0;
        let art = r["artworkUrl100"].as_str().unwrap_or("");
        let key = format!("apple:{id}");
        results.push(json!({
            "kind": "catalog",
            "id": key,
            "title": title,
            "artist": artist,
            "album": r["collectionName"],
            "duration": duration,
            "thumbnail": art.replace("100x100bb", "200x200bb"),
            "in_library": catalog_existing(&key, artist, title).is_some(),
            "catalog": {
                "id": key, "title": title, "artist": artist, "duration": duration,
                "cover": art.replace("100x100bb", "600x600bb"),
            },
        }));
    }
    Ok(results)
}

pub fn catalog_in_library(key: &str, artist: &str, title: &str) -> bool {
    catalog_existing(key, artist, title).is_some()
}

fn catalog_display(title: &str, artist: &str) -> String {
    if artist.is_empty() { title.to_string() } else { format!("{artist} - {title}") }
}

fn catalog_existing(key: &str, artist: &str, title: &str) -> Option<PathBuf> {
    existing_for_source(key).or_else(|| {
        let p = paths().library.join(format!("{}.mp3", sanitize_filename(&catalog_display(title, artist))));
        p.is_file().then_some(p)
    })
}

/// Downloads a catalog song by finding its best audio match on YouTube:
/// among the top results, the one whose length is closest to the song's.
/// Returns (path, display_name, duplicate).
fn download_catalog(item: &Value, job: &Job) -> Result<(PathBuf, String, bool), String> {
    let key = item["id"].as_str().unwrap_or("");
    let title = item["title"].as_str().unwrap_or("").trim().to_string();
    let artist = item["artist"].as_str().unwrap_or("").trim().to_string();
    if key.is_empty() || title.is_empty() {
        return Err("That song is missing its details.".into());
    }
    if let Some(existing) = catalog_existing(key, &artist, &title) {
        record_source(key, &file_name(&existing));
        let stem = file_stem(&existing);
        return Ok((existing, stem, true));
    }

    let query = if artist.is_empty() { format!("{title} audio") } else { format!("{artist} {title} audio") };
    let target = item["duration"].as_f64().unwrap_or(0.0);
    let matches = search_youtube(&query).unwrap_or_default();
    let best = matches
        .iter()
        .filter(|m| target <= 0.0 || (m["duration"].as_f64().unwrap_or(0.0) - target).abs() <= 12.0)
        .min_by(|a, b| {
            let d = |m: &Value| (m["duration"].as_f64().unwrap_or(0.0) - target).abs();
            d(a).partial_cmp(&d(b)).unwrap_or(std::cmp::Ordering::Equal)
        })
        .or_else(|| matches.first());
    let target_url = best
        .and_then(|m| m["url"].as_str().map(String::from))
        .unwrap_or_else(|| format!("ytsearch1:{query}"));

    let info = run_ytdlp(&target_url, Some(job))?;
    let downloaded = final_path(&info)?;
    let display = catalog_display(&title, &artist);
    let final_path = paths().library.join(format!("{}.mp3", sanitize_filename(&display)));
    let same = std::fs::canonicalize(&final_path).ok().zip(std::fs::canonicalize(&downloaded).ok()).is_some_and(|(a, b)| a == b);
    if !same {
        let _ = std::fs::remove_file(&final_path);
        std::fs::rename(&downloaded, &final_path).map_err(|e| e.to_string())?;
    }
    remember_real_names(&final_path, &title, &artist);
    if let Some(cover) = item["cover"].as_str().filter(|c| !c.is_empty()) {
        download_image(cover, &cover_path_for(&final_path));
    }
    record_source(key, &file_name(&final_path));
    if let Some(id) = info["id"].as_str() {
        record_source(&format!("yt:{id}"), &file_name(&final_path));
    }
    Ok((final_path, display, false))
}

/// Starts a download job for a catalog song; returns the job id.
pub fn start_catalog(item: Value) -> String {
    let job = new_download_job("track");
    let id = register(&job);
    let worker_job = job.clone();
    std::thread::spawn(move || {
        let job = worker_job;
        let _ = std::fs::create_dir_all(&paths().library);
        let _ = std::fs::create_dir_all(&paths().covers);
        let mut new_ids: Vec<String> = Vec::new();
        match download_catalog(&item, &job) {
            Ok((path, title, duplicate)) => {
                job.with(|j| {
                    j["duplicate"] = json!(duplicate);
                    j["title"] = json!(title);
                    j["id"] = json!(file_name(&path));
                });
                if !duplicate {
                    job.with(|j| j["percent"] = json!("Fetching lyrics..."));
                    lyrics::fetch_with_song(&path);
                    new_ids.push(file_name(&path));
                }
                job.with(|j| j["status"] = json!("done"));
            }
            Err(e) => {
                store::error(format!("Catalog download failed for {}: {e}", item["title"]));
                job.with(|j| {
                    j["status"] = json!("error");
                    j["error"] = json!(e);
                });
            }
        }
        library::scan_loudness_async();
        lyrics::prefetch_async(new_ids);
    });
    id
}

// ---------------- import jobs ----------------

fn finish_track(job: &Job, playlist_id: Option<&str>, path: &Path) {
    if let Some(pid) = playlist_id {
        add_track_to_playlist_record(pid, &file_name(path));
    }
    job.with(|j| j["percent"] = json!("Fetching lyrics..."));
    lyrics::fetch_with_song(path);
    job.with(|j| {
        j["fraction"] = json!(0.0);
        j["percent"] = json!("");
    });
}

/// Downloads every track of a playlist link. With `as_playlist`, the tracks are
/// also collected into a new local playlist; otherwise they only join the library.
fn run_playlist_import(url: &str, job: &Job, as_playlist: bool) -> Result<(), String> {
    job.with(|j| j["as_playlist"] = json!(as_playlist));

    if is_spotify_playlist_url(url) {
        let (title, track_urls) = spotify::fetch_playlist_tracks(url)?;
        if track_urls.is_empty() {
            return Err("Could not read tracks from that Spotify playlist link.".into());
        }
        let name = if title.is_empty() { "Imported Playlist".to_string() } else { title.clone() };
        let playlist = as_playlist.then(|| create_playlist_record(&name));
        let pid = playlist.as_ref().and_then(|p| p["id"].as_str().map(String::from));
        job.with(|j| {
            j["playlist_id"] = json!(pid);
            j["playlist_name"] = json!(if as_playlist { name.clone() } else if title.is_empty() { "Spotify playlist".into() } else { title.clone() });
            j["total"] = json!(track_urls.len());
        });
        for track_url in &track_urls {
            job.with(|j| j["current_title"] = Value::Null);
            let (path, track_title, duplicate) = match download_spotify(track_url, Some(job)) {
                Ok(r) => r,
                Err(e) => {
                    store::warn(format!("Download failed for {track_url}: {e}"));
                    job.push("failed", json!({"title": track_url, "error": e}));
                    continue;
                }
            };
            job.with(|j| j["current_title"] = json!(track_title));
            if duplicate {
                if let Some(pid) = &pid {
                    add_track_to_playlist_record(pid, &file_name(&path));
                }
                job.with(|j| {
                    j["fraction"] = json!(0.0);
                    j["percent"] = json!("");
                });
            } else {
                finish_track(job, pid.as_deref(), &path);
            }
            job.push(if duplicate { "skipped" } else { "added" }, json!({"id": file_name(&path), "title": track_title}));
        }
        return Ok(());
    }

    let playlist = as_playlist.then(|| create_playlist_record("Importing playlist..."));
    let pid = playlist.as_ref().and_then(|p| p["id"].as_str().map(String::from));
    job.with(|j| {
        j["playlist_id"] = json!(pid);
        j["playlist_name"] = json!(if as_playlist { "Importing playlist..." } else { "YouTube playlist" });
    });
    let (playlist_title, entries) = list_youtube_playlist(url);
    job.with(|j| j["total"] = json!(entries.len()));
    for (id, entry_title) in &entries {
        if let Some(existing) = existing_for_source(&format!("yt:{id}")) {
            // Already downloaded: still belongs in the playlist, just don't re-fetch it.
            if let Some(pid) = &pid {
                add_track_to_playlist_record(pid, &file_name(&existing));
            }
            job.push("skipped", json!({"id": file_name(&existing), "title": file_stem(&existing)}));
            continue;
        }
        job.with(|j| {
            j["current_title"] = json!(entry_title);
            j["fraction"] = json!(0.0);
        });
        match download_youtube(&format!("https://www.youtube.com/watch?v={id}"), Some(job)) {
            Ok((path, title)) => {
                finish_track(job, pid.as_deref(), &path);
                job.push("added", json!({"id": file_name(&path), "title": title}));
            }
            Err(e) => {
                store::warn(format!("Download failed for {id}: {e}"));
                job.push("failed", json!({"title": entry_title, "error": e}));
            }
        }
    }
    let final_name = playlist_title.unwrap_or_else(|| "Imported Playlist".into());
    if let Some(pid) = &pid {
        rename_playlist_record(pid, &final_name);
    }
    job.with(|j| j["playlist_name"] = json!(final_name));
    let empty = job.with(|j| j["added"].as_array().unwrap().is_empty() && j["skipped"].as_array().unwrap().is_empty());
    if empty {
        return Err("Could not read any tracks from that YouTube playlist link.".into());
    }
    Ok(())
}

fn run_single(url: &str, job: &Job) -> Result<Option<String>, String> {
    let (path, title, duplicate) = if is_spotify_url(url) {
        download_spotify(url, Some(job))?
    } else if let Some(existing) = source_key(url).as_deref().and_then(existing_for_source) {
        let stem = file_stem(&existing);
        (existing, stem, true)
    } else {
        let (p, t) = download_youtube(url, Some(job))?;
        (p, t, false)
    };
    job.with(|j| {
        j["duplicate"] = json!(duplicate);
        j["title"] = json!(title);
        j["id"] = json!(file_name(&path));
    });
    if duplicate {
        return Ok(None);
    }
    job.with(|j| j["percent"] = json!("Fetching lyrics..."));
    lyrics::fetch_with_song(&path);
    Ok(Some(file_name(&path)))
}

/// Starts a download job on a background thread; returns its id.
pub fn start(url: String, mode: String, as_playlist: bool) -> String {
    let job = new_download_job(if mode == "playlist" { "playlist" } else { "track" });
    let id = register(&job);
    let worker_job = job.clone();
    std::thread::spawn(move || {
        let job = worker_job;
        let _ = std::fs::create_dir_all(&paths().library);
        let _ = std::fs::create_dir_all(&paths().covers);
        let mut new_ids: Vec<String> = Vec::new(); // songs this job added, for a catch-up lyrics pass
        let result = if mode == "playlist" {
            run_playlist_import(&url, &job, as_playlist)
        } else {
            run_single(&url, &job).map(|added| new_ids.extend(added))
        };
        match result {
            Ok(()) => job.with(|j| j["status"] = json!("done")),
            Err(e) => {
                store::error(format!("Download job failed for {url}: {e}"));
                job.with(|j| {
                    j["status"] = json!("error");
                    j["error"] = json!(e);
                });
            }
        }
        if mode == "playlist" {
            new_ids = job.with(|j| {
                j["added"].as_array().unwrap().iter().filter_map(|i| i["id"].as_str().map(String::from)).collect()
            });
        }
        library::scan_loudness_async(); // measure whatever was just added
        lyrics::prefetch_async(new_ids); // retries any lookups that failed while offline
    });
    id
}
