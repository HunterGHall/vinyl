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
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

// ---------------- finding duplicates ----------------
// Two kinds: byte-identical files (whatever they're called), and the same song
// saved twice - same title (ignoring "(feat. ...)"-style suffixes), same lead
// artist, and nearly the same length. Merging keeps one copy and moves the
// others' play counts and playlist spots onto it.

// A file's hash only changes if the file does, so scans after the first reuse it: the size and
// modified time are checked, and a changed file is hashed again.
static HASH_CACHE: LazyLock<Mutex<HashMap<PathBuf, (u64, std::time::SystemTime, String)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn cached_sha1(path: &Path) -> std::io::Result<String> {
    let meta = std::fs::metadata(path)?;
    let stamp = (meta.len(), meta.modified().ok());
    if let (Some(mtime), Some((size, cached_mtime, hash))) = (stamp.1, HASH_CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(path)) {
        if *size == stamp.0 && *cached_mtime == mtime {
            return Ok(hash.clone());
        }
    }
    let hash = file_sha1(path)?;
    if let Some(mtime) = stamp.1 {
        HASH_CACHE.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), (stamp.0, mtime, hash.clone()));
    }
    Ok(hash)
}

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
            if let Ok(h) = cached_sha1(&items[i].path) {
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
