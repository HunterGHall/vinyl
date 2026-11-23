//! Library tools: duplicate finding/merging, name fixing, and the Spotify
//! playlist watcher.

use crate::music::download::{self, Job, youtube_track_fields};
use crate::library::*;
use crate::music::lyrics::{clean_title, first_artist};
use crate::base::store::{self, lock, paths, save};
use crate::music::spotify;
use crate::base::util::{self, words};
use regex_lite::Regex;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::Duration;

// ---------------- finding duplicates ----------------
// Two kinds: byte-identical files (whatever they're called), and the same song
// saved twice - same title (ignoring "(feat. ...)"-style suffixes), same lead
// artist, and nearly the same length. Merging keeps one copy and moves the
// others' play counts and playlist spots onto it.

fn file_sha1(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

struct Item {
    name: String,
    path: std::path::PathBuf,
    title: String,
    artist: String,
    duration: f64,
    size: u64,
    title_key: String,
    artist_key: String,
}

fn find_root(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

pub fn find_duplicate_groups() -> Vec<Value> {
    let (meta, stats, playlists) = (load_meta(), load_stats(), load_playlists());
    let tracks = find_track_paths();
    warm_durations(&tracks);
    let items: Vec<Item> = tracks
        .into_iter()
        .map(|path| {
            let (title, artist) = name_fields(&path, &meta);
            Item {
                name: file_name(&path),
                duration: track_duration(&path),
                size: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                title_key: words(&clean_title(&title)),
                artist_key: words(&first_artist(&artist)),
                title,
                artist,
                path,
            }
        })
        .collect();
    let mut parent: Vec<usize> = (0..items.len()).collect();
    let union = |parent: &mut Vec<usize>, a: usize, b: usize| {
        let (ra, rb) = (find_root(parent, a), find_root(parent, b));
        parent[ra] = rb;
    };

    // 1) identical bytes - only hash files that share a size
    let mut hashes: HashMap<usize, String> = HashMap::new();
    let mut by_size: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        by_size.entry(it.size).or_default().push(i);
    }
    for idxs in by_size.values().filter(|v| v.len() > 1) {
        let mut by_hash: HashMap<String, Vec<usize>> = HashMap::new();
        for &i in idxs {
            if let Ok(h) = file_sha1(&items[i].path) {
                hashes.insert(i, h.clone());
                by_hash.entry(h).or_default().push(i);
            }
        }
        for same in by_hash.values() {
            for &other in &same[1..] {
                union(&mut parent, same[0], other);
            }
        }
    }

    // 2) the same song under a different name
    let mut by_title: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        if !it.title_key.is_empty() {
            by_title.entry(&it.title_key).or_default().push(i);
        }
    }
    for idxs in by_title.values() {
        for (n, &a) in idxs.iter().enumerate() {
            for &b in &idxs[n + 1..] {
                let (x, y) = (&items[a], &items[b]);
                if x.duration == 0.0 || y.duration == 0.0 || (x.duration - y.duration).abs() > 3.0 {
                    continue;
                }
                if !x.artist_key.is_empty() && !y.artist_key.is_empty() && x.artist_key != y.artist_key {
                    continue;
                }
                union(&mut parent, a, b);
            }
        }
    }

    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut group_of: HashMap<usize, usize> = HashMap::new();
    for i in 0..items.len() {
        let root = find_root(&mut parent, i);
        let g = *group_of.entry(root).or_insert_with(|| {
            groups.push((root, vec![]));
            groups.len() - 1
        });
        groups[g].1.push(i);
    }

    let mut playlist_count: HashMap<String, i64> = HashMap::new();
    for pl in playlists.as_array().into_iter().flatten() {
        for t in track_ids_of(pl) {
            *playlist_count.entry(t).or_default() += 1;
        }
    }

    let mut result: Vec<Value> = Vec::new();
    for (_, mut idxs) in groups.into_iter().filter(|(_, v)| v.len() > 1) {
        // keep the most-played, then most-playlisted, then biggest (best quality),
        // then the one with the fuller name (more credited artists)
        idxs.sort_by_key(|&i| {
            let n = &items[i].name;
            (
                Reverse(stats[n].as_i64().unwrap_or(0)),
                Reverse(playlist_count.get(n).copied().unwrap_or(0)),
                Reverse(items[i].size),
                Reverse(n.chars().count()),
            )
        });
        let hs: Vec<Option<&String>> = idxs.iter().map(|i| hashes.get(i)).collect();
        let identical = hs.iter().all(|h| h.is_some()) && hs.iter().all(|h| *h == hs[0]);
        let files: Vec<Value> = idxs
            .iter()
            .enumerate()
            .map(|(n, &i)| {
                let it = &items[i];
                json!({
                    "id": it.name, "title": it.title, "artist": it.artist,
                    "duration": it.duration, "size": it.size,
                    "plays": stats[&it.name].as_i64().unwrap_or(0),
                    "playlists": playlist_count.get(&it.name).copied().unwrap_or(0),
                    "cover": cover_url(&it.path),
                    "keep": n == 0,
                })
            })
            .collect();
        result.push(json!({"reason": if identical { "identical" } else { "same-song" }, "files": files}));
    }
    result.sort_by_key(|g| g["files"][0]["title"].as_str().unwrap_or("").to_lowercase());
    for (i, g) in result.iter_mut().enumerate() {
        g["id"] = json!(i);
    }
    result
}

/// `{groups: [{keep, remove: [ids]}]}` - deletes the `remove` copies, moving
/// their play counts, playlist spots and import sources onto `keep`.
/// Returns (removed, deleted).
pub fn merge_duplicates(groups: &[Value]) -> (usize, usize) {
    let mut removed: Vec<String> = Vec::new();
    {
        let _g = lock();
        let (mut playlists, mut stats, mut sources) = (load_playlists(), load_stats(), load_sources());
        for group in groups.iter().filter(|g| g.is_object()) {
            let Some(keep) = group["keep"].as_str() else { continue };
            if !safe_track_path(keep).is_some_and(|p| p.is_file()) {
                continue;
            }
            let drop: Vec<String> = group["remove"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .filter(|t| *t != keep && safe_track_path(t).is_some_and(|p| p.is_file()))
                .map(String::from)
                .collect();
            if drop.is_empty() {
                continue;
            }
            let gone: HashSet<&str> = drop.iter().map(|s| s.as_str()).collect();
            for pl in playlists.as_array_mut().unwrap() {
                let ids = track_ids_of(pl);
                if !ids.iter().any(|t| gone.contains(t.as_str())) {
                    continue;
                }
                let mut seen = HashSet::new();
                let merged: Vec<Value> = ids
                    .into_iter()
                    .map(|t| if gone.contains(t.as_str()) { keep.to_string() } else { t })
                    .filter(|t| seen.insert(t.clone()))
                    .map(Value::from)
                    .collect();
                pl["track_ids"] = Value::Array(merged);
            }
            let total = stats[keep].as_i64().unwrap_or(0) + drop.iter().map(|t| stats[t].as_i64().unwrap_or(0)).sum::<i64>();
            if total != 0 {
                stats[keep] = json!(total);
            }
            if let Some(o) = sources.as_object_mut() {
                for v in o.values_mut() {
                    if v.as_str().is_some_and(|n| gone.contains(n)) {
                        *v = json!(keep);
                    }
                }
            }
            removed.extend(drop);
        }
        save_playlists(&playlists);
        save_stats(&stats);
        save(&paths().sources, &sources);
    }
    let deleted = if removed.is_empty() { 0 } else { delete_tracks(&removed) };
    (removed.len(), deleted)
}

// ---------------- fixing old names ----------------
// Songs saved before real titles were kept can show "AC_DC" instead of "AC/DC"
// (files can't hold "/"), or have no artist at all. A scan finds them and
// suggests corrections, which the user reviews before anything changes.

static SPACED_UNDERSCORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s_\s").unwrap());

fn needs_attention(title: &str, artist: &str) -> bool {
    artist.is_empty() || title.contains('_') || artist.contains('_') || title.contains('\u{29f8}') || artist.contains('\u{29f8}')
}

/// Best guess at undoing sanitizing: "A _ B" -> "A / B", "AC_DC" -> "AC/DC".
fn guess_clean(text: &str) -> String {
    let s = text.replace('\u{29f8}', "/");
    let s = SPACED_UNDERSCORE.replace_all(&s, " / ").into_owned();
    let chars: Vec<char> = s.chars().collect();
    (0..chars.len())
        .map(|i| {
            let word = |c: char| c.is_alphanumeric();
            if chars[i] == '_' && i > 0 && i + 1 < chars.len() && word(chars[i - 1]) && word(chars[i + 1]) {
                '/'
            } else {
                chars[i]
            }
        })
        .collect()
}

/// (title, artist) re-read from where the song came from, or None.
fn real_names_from_source(key: &str) -> Result<Option<(String, String)>, String> {
    let (kind, ident) = key.split_once(':').unwrap_or((key, ""));
    match kind {
        "spotify" => {
            let (title, artist, _) = spotify::fetch_track_metadata(&format!("https://open.spotify.com/track/{ident}"))?;
            Ok((!title.is_empty()).then(|| (title.trim().to_string(), artist.trim().to_string())))
        }
        "yt" => {
            let out = util::command("yt-dlp")
                .args(["--skip-download", "--no-playlist", "--no-warnings", "--dump-json"])
                .arg(format!("https://www.youtube.com/watch?v={ident}"))
                .stdin(std::process::Stdio::null())
                .output()
                .map_err(|e| e.to_string())?;
            let info: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
            let (title, artist) = youtube_track_fields(&info);
            Ok((!title.is_empty()).then_some((title, artist)))
        }
        _ => Ok(None),
    }
}

/// (title, artist, reason) for a better name, or None. A suggestion must only
/// differ from the current name by punctuation (or fill in a missing artist) -
/// never replace it with different words.
fn suggest_name(source_key: Option<&str>, title: &str, artist: &str) -> Option<(String, String, &'static str)> {
    let mut candidates: Vec<(String, String, &'static str)> = Vec::new();
    if let Some(key) = source_key {
        match real_names_from_source(key) {
            Ok(Some((t, a))) => candidates.push((t, a, "source")),
            Ok(None) => {}
            Err(e) => store::warn(format!("Couldn't read original names for {key}: {e}")),
        }
    }
    candidates.push((guess_clean(title), guess_clean(artist), "guess"));
    for (nt, na, reason) in candidates {
        if (nt.as_str(), na.as_str()) == (title, artist) {
            continue;
        }
        if words(&nt) != words(title) {
            continue;
        }
        if !artist.is_empty() && words(&na) != words(artist) {
            continue;
        }
        return Some((nt, na, reason));
    }
    None
}

fn scan_names(job: &Job) {
    let by_file: HashMap<String, String> = {
        let _g = lock();
        load_sources()
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| v.as_str().map(|n| (n.to_string(), k.clone())))
            .collect()
    };
    let meta = load_meta();
    let mut candidates = Vec::new();
    for path in find_track_paths() {
        let entry = &meta[file_name(&path)];
        if entry.is_object() && !entry["auto"].as_bool().unwrap_or(false) {
            continue; // the user already edited this one
        }
        let (title, artist) = name_fields(&path, &meta);
        if needs_attention(&title, &artist) {
            candidates.push((path, title, artist));
        }
    }
    job.with(|j| j["total"] = json!(candidates.len()));
    for (path, title, artist) in candidates {
        job.with(|j| j["current"] = json!(file_stem(&path)));
        let suggestion = suggest_name(by_file.get(&file_name(&path)).map(|s| s.as_str()), &title, &artist);
        job.with(|j| {
            match suggestion {
                Some((st, sa, reason)) => j["suggestions"].as_array_mut().unwrap().push(json!({
                    "id": file_name(&path),
                    "current": {"title": title, "artist": artist},
                    "suggested": {"title": st, "artist": sa},
                    "reason": reason,
                })),
                None => j["unresolved"] = json!(j["unresolved"].as_i64().unwrap_or(0) + 1),
            }
            j["checked"] = json!(j["checked"].as_i64().unwrap_or(0) + 1);
        });
    }
    job.with(|j| {
        j["status"] = json!("done");
        j["current"] = Value::Null;
    });
}

pub fn start_name_scan() -> String {
    let job = Job::new(json!({
        "status": "running", "total": 0, "checked": 0, "current": null,
        "suggestions": [], "unresolved": 0, "error": null,
    }));
    let id = download::register(&job);
    std::thread::spawn(move || {
        let guard = job.clone();
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scan_names(&job))).is_err() {
            store::error("Name scan failed");
            guard.with(|j| {
                j["status"] = json!("error");
                j["error"] = json!("Name scan failed");
                j["current"] = Value::Null;
            });
        }
    });
    id
}

// ---------------- spotify playlist watch ----------------
// Every SYNC_INTERVAL seconds the watched Spotify playlist is re-read. Any track
// that isn't in the library and hasn't been seen before becomes "pending" and
// the UI asks whether to add it. The first read after choosing a playlist only
// records what's already there, so you aren't asked about its whole backlog.

const SYNC_INTERVAL: Duration = Duration::from_secs(60);
static SYNC_WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

pub fn wake_sync() {
    *SYNC_WAKE.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
    SYNC_WAKE.1.notify_all();
}

pub fn load_sync() -> Value {
    store::read_obj(&paths().sync)
}

fn save_sync(state: &Value) {
    save(&paths().sync, state);
}

/// One pass over the watched playlist. Network work happens outside the lock.
fn sync_check() {
    let url = load_settings()["spotify_sync_url"].as_str().unwrap_or("").to_string();
    if url.is_empty() {
        return;
    }
    let (track_urls, error) = match spotify::fetch_playlist_tracks(&url) {
        Ok((_, urls)) if urls.is_empty() => (vec![], Some("Could not read any tracks from that playlist.".to_string())),
        Ok((_, urls)) => (urls, None),
        Err(e) => {
            store::warn(format!("Spotify sync: couldn't read the playlist: {e}"));
            (vec![], Some(format!("Could not reach Spotify: {e}")))
        }
    };

    let (first_run, mut seen, pending, sources) = {
        let _g = lock();
        let mut state = load_sync();
        if state["url"].as_str() != Some(&url) {
            state = json!({"url": url});
        }
        state["last_checked"] = json!(store::now());
        state["error"] = json!(error);
        save_sync(&state);
        if error.is_some() {
            return;
        }
        let first_run = state.get("seen").is_none();
        let seen = state["seen"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect::<BTreeSet<_>>();
        let pending = state["pending"].as_array().cloned().unwrap_or_default();
        (first_run, seen, pending, load_sources())
    };

    let ids: Vec<&str> = track_urls.iter().map(|u| u.rsplit('/').next().unwrap_or("")).collect();
    let mut new_ids: Vec<&str> = Vec::new();
    if first_run {
        seen.extend(ids.iter().map(|s| s.to_string()));
    } else {
        let queued: HashSet<&str> = pending.iter().filter_map(|p| p["id"].as_str()).collect();
        for tid in ids {
            if seen.contains(tid) || queued.contains(tid) {
                continue;
            }
            let downloaded = sources[format!("spotify:{tid}")].as_str().is_some_and(|n| paths().library.join(n).is_file());
            if downloaded {
                seen.insert(tid.to_string()); // already downloaded some other way
            } else {
                new_ids.push(tid);
            }
        }
    }

    let found: Vec<Value> = new_ids
        .iter()
        .map(|tid| {
            let track_url = format!("https://open.spotify.com/track/{tid}");
            let (title, artist, cover) = spotify::fetch_track_metadata(&track_url).unwrap_or_else(|e| {
                store::warn(format!("Spotify sync: no metadata for {tid}: {e}"));
                (String::new(), String::new(), None)
            });
            json!({"id": tid, "url": track_url,
                   "title": if title.is_empty() { "Unknown title".to_string() } else { title },
                   "artist": artist, "cover": cover})
        })
        .collect();

    let _g = lock();
    let mut state = load_sync();
    if state["url"].as_str() != Some(&url) {
        return; // playlist changed while we were reading; next pass starts over
    }
    let mut all_seen: BTreeSet<String> =
        state["seen"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    all_seen.extend(seen);
    state["seen"] = json!(all_seen);
    let mut pending_now = state["pending"].as_array().cloned().unwrap_or_default();
    let known: HashSet<String> = pending_now.iter().filter_map(|p| p["id"].as_str().map(String::from)).collect();
    pending_now.extend(found.into_iter().filter(|f| !known.contains(f["id"].as_str().unwrap_or(""))));
    state["pending"] = Value::Array(pending_now);
    state["last_checked"] = json!(store::now());
    state["error"] = Value::Null;
    save_sync(&state);
}

pub fn start_sync_thread() {
    std::thread::spawn(|| {
        loop {
            if std::panic::catch_unwind(sync_check).is_err() {
                store::error("Spotify sync failed");
            }
            let (flag, cv) = &SYNC_WAKE;
            let guard = flag.lock().unwrap_or_else(|e| e.into_inner());
            let (mut guard, _) = cv
                .wait_timeout_while(guard, SYNC_INTERVAL, |woken| !*woken)
                .unwrap_or_else(|e| e.into_inner());
            *guard = false;
        }
    });
}

/// Takes a pending track off the list (it was added, or the user skipped it).
pub fn resolve_pending(track_id: &str) {
    let _g = lock();
    let mut state = load_sync();
    let pending: Vec<Value> = state["pending"].as_array().cloned().unwrap_or_default().into_iter().filter(|p| p["id"] != track_id).collect();
    state["pending"] = Value::Array(pending);
    let mut seen: BTreeSet<String> =
        state["seen"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
    seen.insert(track_id.to_string());
    state["seen"] = json!(seen);
    save_sync(&state);
}
