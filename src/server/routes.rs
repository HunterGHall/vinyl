//! The HTTP server: static files, media, and the JSON API the frontend talks to.

use crate::library::*;
use crate::base::store::{self, lock, paths};
use crate::base::util::{clip, is_spotify_playlist_url, unquote};
use crate::system::{deps, mediakeys, update};
use crate::music::{discover, download, lyrics};
use crate::library::tools;
use serde_json::{Map, Value, json};
use std::fs;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tiny_http::{Header, Method, Request, Response, Server};

type Resp = Response<Box<dyn Read + Send + 'static>>;

// ---------------- responses ----------------

fn hdr(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn bytes_resp(status: u16, content_type: &str, data: Vec<u8>) -> Resp {
    let len = data.len();
    Response::new(
        status.into(),
        vec![hdr("Content-Type", content_type)],
        Box::new(Cursor::new(data)),
        Some(len),
        None,
    )
}

fn json_resp(status: u16, v: Value) -> Resp {
    bytes_resp(status, "application/json", serde_json::to_vec(&v).unwrap_or_default())
}

fn ok(v: Value) -> Resp {
    json_resp(200, v)
}

fn empty(status: u16) -> Resp {
    bytes_resp(status, "text/plain", Vec::new())
}

fn err(status: u16, msg: &str) -> Resp {
    json_resp(status, json!({"error": msg}))
}

fn not_found() -> Resp {
    err(404, "Not found")
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") | Some("map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/mp4",
        Some("flac") => "audio/flac",
        Some("ogg") | Some("opus") => "audio/ogg",
        Some("aac") => "audio/aac",
        _ => "application/octet-stream",
    }
}

/// Parses `bytes=START-END` / `bytes=START-` / `bytes=-N` against a file length.
fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
    let spec = header.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        (len.saturating_sub(n), len.checked_sub(1)?)
    } else {
        let start: u64 = a.parse().ok()?;
        let end = if b.is_empty() { len.checked_sub(1)? } else { b.parse::<u64>().ok()?.min(len.checked_sub(1)?) };
        (start, end)
    };
    (start <= end && start < len).then_some((start, end))
}

/// Serves a file, honouring `Range` requests (needed for seeking in audio).
/// Cover art URLs carry `?v=<modified time>`, so a changed cover is a new URL and the old one can be kept forever.
const CACHE_FOREVER: &str = "public, max-age=31536000, immutable";
const CACHE_REVALIDATE: &str = "no-cache";
const CACHE_NEVER: &str = "no-store, must-revalidate";

fn serve_file(path: &Path, range: Option<&str>, cache: &str) -> Resp {
    let Ok(mut file) = fs::File::open(path) else { return not_found() };
    let Ok(len) = file.metadata().map(|m| m.len()) else { return not_found() };
    let mut headers = vec![
        hdr("Content-Type", mime_for(path)),
        hdr("Accept-Ranges", "bytes"),
        hdr("Cache-Control", cache),
    ];
    let (status, start, count) = match range {
        Some(r) => match parse_range(r, len) {
            Some((s, e)) => {
                headers.push(hdr("Content-Range", &format!("bytes {s}-{e}/{len}")));
                (206u16, s, e - s + 1)
            }
            None => {
                return Response::new(
                    416.into(),
                    vec![hdr("Content-Range", &format!("bytes */{len}"))],
                    Box::new(Cursor::new(Vec::new())),
                    Some(0),
                    None,
                );
            }
        },
        None => (200, 0, len),
    };
    if start > 0 && file.seek(SeekFrom::Start(start)).is_err() {
        return not_found();
    }
    Response::new(status.into(), headers, Box::new(file.take(count)), Some(count as usize), None)
}

/// A file under `root` named by URL path segments; None if it would escape `root`.
fn under(root: &Path, segs: &[&str]) -> Option<PathBuf> {
    let mut p = root.to_path_buf();
    for s in segs {
        if s.is_empty() || *s == "." || *s == ".." || s.contains(['\\', ':', '\0']) {
            return None;
        }
        p.push(s);
    }
    Some(p)
}

// ---------------- request helpers ----------------

fn body_obj(body: &[u8]) -> Value {
    match serde_json::from_slice::<Value>(body) {
        Ok(v) if v.is_object() => v,
        _ => Value::Object(Map::new()),
    }
}

fn str_list(v: &Value) -> Option<Vec<String>> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

// ---------------- the router ----------------

struct Ctx<'a> {
    if_none_match: Option<&'a str>,
    body: &'a [u8],
    query: &'a str,
    range: Option<&'a str>,
}

fn route(method: Method, path: &str, cx: &Ctx) -> Resp {
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let b = || body_obj(cx.body);
    let p = paths();
    match (method, segs.as_slice()) {
        // ---- static + media ----
        (Method::Get, [""]) => static_resp("index.html", cx),
        (Method::Get, ["covers", rest @ ..]) => under(&p.covers, rest).map_or_else(not_found, |f| serve_file(&f, None, CACHE_FOREVER)),
        (Method::Get, ["playlist-covers", rest @ ..]) => {
            under(&p.playlist_covers, rest).map_or_else(not_found, |f| serve_file(&f, None, CACHE_FOREVER))
        }
        (Method::Get, ["audio", name]) => match safe_track_path(name) {
            Some(f) if f.is_file() => serve_file(&f, cx.range, CACHE_REVALIDATE),
            _ => not_found(),
        },

        // ---- library ----
        (Method::Get, ["api", "library"]) => bytes_resp(200, "application/json", library_json().to_vec()),
        (Method::Patch, ["api", "tracks", id, "meta"]) => edit_track_meta(id, &b()),
        (Method::Get, ["api", "tracks", id, "loudness"]) => match safe_track_path(id) {
            Some(f) if f.is_file() => ok(json!({"gain_db": ensure_loudness(&f)})),
            _ => not_found(),
        },
        (Method::Post, ["api", "tracks", id, "play"]) => ok(json!({"plays": record_play(id)})),
        (Method::Delete, ["api", "tracks"]) => delete_all_tracks(),
        (Method::Post, ["api", "tracks", "batch-delete"]) => match str_list(&b()["track_ids"]).filter(|v| !v.is_empty()) {
            Some(ids) => ok(json!({"deleted": delete_tracks(&ids)})),
            None => err(400, "track_ids required"),
        },
        (Method::Post, ["api", "tracks", "batch-meta"]) => batch_meta(&b()),
        (Method::Delete, ["api", "tracks", id @ ..]) => {
            delete_tracks(&[id.join("/")]);
            empty(204)
        }

        // ---- likes + history ----
        (Method::Get, ["api", "likes"]) => ok(load_likes()),
        (Method::Put, ["api", "likes", id]) => match safe_track_path(id).filter(|f| f.is_file()) {
            Some(_) => ok(json!({"liked": set_liked(id, true)})),
            None => not_found(),
        },
        (Method::Delete, ["api", "likes", id]) => ok(json!({"liked": set_liked(id, false)})),
        (Method::Get, ["api", "history"]) => ok(load_history()),
        // Reset what the recommendations learn from: likes, listening history and play counts.
        (Method::Delete, ["api", "history"]) => {
            let _g = lock();
            let _ = store::write_json(&paths().likes, &json!([]), true, true);
            clear_history();
            save_stats(&json!({}));
            empty(204)
        }

        // ---- lyrics ----
        (Method::Post, ["api", "tracks", id, "lyrics", "offset"]) => lyrics_offset(id, &b()),
        (Method::Get, ["api", "tracks", id, "lyrics"]) => track_lyrics(id, cx.query_param("refresh") == Some("1")),

        // ---- duplicates / names ----
        (Method::Get, ["api", "duplicates"]) => {
            let groups = tools::find_duplicate_groups();
            let extra: usize = groups.iter().map(|g| g["files"].as_array().map_or(0, |f| f.len() - 1)).sum();
            ok(json!({"groups": groups, "extra_files": extra}))
        }
        (Method::Post, ["api", "duplicates", "merge"]) => match b()["groups"].as_array().filter(|g| !g.is_empty()) {
            Some(groups) => {
                let (removed, deleted) = tools::merge_duplicates(groups);
                ok(json!({"removed": removed, "deleted": deleted}))
            }
            None => err(400, "groups required"),
        },
        (Method::Post, ["api", "fix-names", "scan"]) => json_resp(202, json!({"job_id": tools::start_name_scan()})),
        (Method::Get, ["api", "fix-names", job]) => download::lookup(job).map_or_else(not_found, |j| ok(j.snapshot())),

        // ---- player state + settings ----
        (Method::Get, ["api", "state"]) => ok(store::read_obj(&p.player_state)),
        (Method::Post, ["api", "state"]) => {
            if cx.body.len() > 2_000_000 {
                return err(413, "Too large");
            }
            match serde_json::from_slice::<Value>(cx.body) {
                Ok(v) if v.is_object() => {
                    let _ = store::write_json(&p.player_state, &v, false, false);
                    empty(204)
                }
                _ => err(400, "Object required"),
            }
        }
        (Method::Get, ["api", "settings"]) => ok(load_settings()),
        (Method::Post, ["api", "settings"]) => update_settings(&b()),

        // ---- spotify watch ----
        (Method::Get, ["api", "sync"]) => {
            let (url, state) = {
                let _g = lock();
                (load_settings()["spotify_sync_url"].as_str().unwrap_or("").to_string(), tools::load_sync())
            };
            if url.is_empty() || state["url"].as_str() != Some(&url) {
                ok(json!({"url": url, "pending": [], "last_checked": null, "error": null}))
            } else {
                ok(json!({"url": url, "pending": state["pending"].as_array().cloned().unwrap_or_default(),
                          "last_checked": state["last_checked"], "error": state["error"]}))
            }
        }
        (Method::Post, ["api", "sync", "check"]) => {
            tools::wake_sync();
            empty(204)
        }
        (Method::Post, ["api", "sync", id, "resolve"]) => {
            tools::resolve_pending(id);
            empty(204)
        }

        // ---- playlists ----
        (Method::Get, ["api", "playlists"]) => {
            let _g = lock();
            ok(Value::Array(load_playlists().as_array().unwrap().iter().map(playlist_public).collect()))
        }
        (Method::Put, ["api", "playlists", "order"]) => reorder_playlists(&b()),
        (Method::Post, ["api", "playlists"]) => {
            let name = b()["name"].as_str().unwrap_or("").trim().to_string();
            if name.is_empty() {
                return err(400, "Name required");
            }
            json_resp(201, create_playlist_record(&name))
        }
        (Method::Delete, ["api", "playlists"]) => {
            {
                let _g = lock();
                save_playlists(&json!([]));
            }
            let _ = fs::remove_dir_all(&p.playlist_covers);
            empty(204)
        }
        (Method::Patch, ["api", "playlists", pid]) => update_playlist(pid, &b()),
        (Method::Post, ["api", "playlists", pid, "cover"]) => set_playlist_cover(pid, cx.body),
        (Method::Delete, ["api", "playlists", pid, "cover"]) => {
            let _g = lock();
            if find_playlist(&mut load_playlists(), pid).is_none() {
                return not_found();
            }
            let _ = fs::remove_file(playlist_cover_path(pid));
            empty(204)
        }
        (Method::Delete, ["api", "playlists", pid]) => {
            let _g = lock();
            let playlists = load_playlists();
            let remaining: Vec<Value> = playlists.as_array().unwrap().iter().filter(|p| p["id"] != *pid).cloned().collect();
            if remaining.len() == playlists.as_array().unwrap().len() {
                return not_found();
            }
            save_playlists(&Value::Array(remaining));
            let _ = fs::remove_file(playlist_cover_path(pid));
            empty(204)
        }
        (Method::Put, ["api", "playlists", pid, "order"]) => edit_playlist(pid, |pl| {
            let ids = str_list(&b()["track_ids"]).ok_or((400, "track_ids required"))?;
            // Must be a pure reordering - never add or drop tracks.
            let (mut a, mut c) = (ids.clone(), track_ids_of(pl));
            a.sort();
            c.sort();
            if a != c {
                return Err((400, "track_ids must match the playlist's tracks"));
            }
            pl["track_ids"] = json!(ids);
            Ok(())
        }),
        (Method::Post, ["api", "playlists", pid, "tracks", "batch-remove"]) => edit_playlist(pid, |pl| {
            let drop = str_list(&b()["track_ids"]).filter(|v| !v.is_empty()).ok_or((400, "track_ids required"))?;
            let kept: Vec<String> = track_ids_of(pl).into_iter().filter(|t| !drop.contains(t)).collect();
            pl["track_ids"] = json!(kept);
            Ok(())
        }),
        (Method::Post, ["api", "playlists", pid, "tracks"]) => edit_playlist(pid, |pl| {
            let body = b();
            let ids = match body.get("track_ids") {
                Some(v) => str_list(v),
                None => body["track_id"].as_str().filter(|s| !s.is_empty()).map(|s| vec![s.to_string()]),
            }
            .filter(|v| !v.is_empty())
            .ok_or((400, "track_id required"))?;
            let mut current = track_ids_of(pl);
            for id in ids {
                if !current.contains(&id) {
                    current.push(id);
                }
            }
            pl["track_ids"] = json!(current);
            Ok(())
        }),
        (Method::Delete, ["api", "playlists", pid, "tracks", id @ ..]) => edit_playlist(pid, |pl| {
            let id = id.join("/");
            let kept: Vec<String> = track_ids_of(pl).into_iter().filter(|t| *t != id).collect();
            pl["track_ids"] = json!(kept);
            Ok(())
        }),

        // ---- downloads ----
        (Method::Post, ["api", "download"]) => {
            let body = b();
            let mode = body["mode"].as_str().unwrap_or("track").to_string();
            if mode == "catalog" {
                let item = body["catalog"].clone();
                if !item.is_object() {
                    return err(400, "catalog song required");
                }
                return json_resp(202, json!({"job_id": download::start_catalog(item)}));
            }
            let url = body["url"].as_str().unwrap_or("").trim().to_string();
            if url.is_empty() {
                return err(400, "URL required");
            }
            let as_playlist = body["as_playlist"] != json!(false);
            json_resp(202, json!({"job_id": download::start(url, mode, as_playlist)}))
        }
        (Method::Get, ["api", "search"]) => {
            let q = unquote(&cx.query_param("q").unwrap_or("").replace('+', " "));
            let q = clip(q.trim(), 120);
            if q.is_empty() {
                return err(400, "Enter a song or artist to search for");
            }
            let found = if cx.query_param("source") == Some("youtube") {
                download::search_youtube(&q)
            } else {
                download::search_catalog(&q)
            };
            match found {
                Ok(results) => ok(json!({"results": results})),
                Err(e) => err(502, &e),
            }
        }
        // ---- media keys (desktop window only): long-poll for key presses ----
        (Method::Get, ["api", "media", "poll"]) => {
            if !mediakeys::enabled() {
                return ok(json!({"enabled": false, "keys": []}));
            }
            ok(json!({"enabled": true, "keys": mediakeys::wait(std::time::Duration::from_secs(20))}))
        }

        // ---- download tools (yt-dlp, ffmpeg) ----
        (Method::Get, ["api", "deps"]) => ok(deps::status()),
        (Method::Post, ["api", "deps", "install"]) => json_resp(202, json!({"started": deps::install()})),

        // ---- app updates ----
        (Method::Get, ["api", "update"]) => match update::check() {
            Ok(v) => ok(v),
            Err(e) => err(502, &e),
        },
        (Method::Get, ["api", "update", "status"]) => ok(update::status()),
        (Method::Post, ["api", "update", "install"]) => json_resp(202, json!({"started": update::install()})),

        // ---- discovery (Deezer) ----
        (Method::Get, ["api", "discover", what]) => {
            let q = |k: &str| unquote(&cx.query_param(k).unwrap_or("").replace('+', " "));
            let num = |k: &str, d: usize| q(k).parse::<usize>().unwrap_or(d);
            let res = match *what {
                "similar" => discover::similar(&clip(q("artist").trim(), 80), num("n", 6), num("per", 2)),
                "artist" => discover::artist(q("id").parse().unwrap_or(0)),
                "genres" => discover::genres(),
                "genre" => discover::genre(q("id").parse().unwrap_or(0)),
                _ => return not_found(),
            };
            match res {
                Ok(v) => ok(v),
                Err(e) => err(502, &e),
            }
        }
        (Method::Get, ["api", "download", job]) => download::lookup(job).map_or_else(not_found, |j| ok(j.snapshot())),

        // ---- anything else: the frontend's static files ----
        (Method::Get, rest) => static_resp(&rest.join("/"), cx),
        _ => not_found(),
    }
}

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

/// A frontend file: from the copy packed into the executable, or (when
/// `MUSIC_DEV_STATIC` is set) straight from `static/` so edits show on reload.
fn static_resp(rel: &str, cx: &Ctx) -> Resp {
    if std::env::var_os("MUSIC_DEV_STATIC").is_some() {
        let segs: Vec<&str> = rel.split('/').collect();
        return under(&paths().static_dir, &segs)
            .filter(|f| f.is_file())
            .map_or_else(not_found, |f| serve_file(&f, None, CACHE_NEVER));
    }
    let key = format!("/{rel}");
    let Some((_, gz, etag)) = embedded::ASSETS.iter().find(|(p, _, _)| *p == key) else { return not_found() };
    if cx.if_none_match == Some(*etag) {
        return Response::new(304.into(), vec![hdr("ETag", etag)], Box::new(Cursor::new(Vec::new())), Some(0), None);
    }
    Response::new(
        200.into(),
        vec![
            hdr("Content-Type", mime_for(Path::new(rel))),
            hdr("Content-Encoding", "gzip"),
            hdr("ETag", etag),
            hdr("Cache-Control", "no-cache"),
        ],
        Box::new(Cursor::new(gz.to_vec())),
        Some(gz.len()),
        None,
    )
}

impl Ctx<'_> {
    fn query_param(&self, key: &str) -> Option<&str> {
        query_param(self.query, key)
    }
}

// ---------------- handlers ----------------

fn edit_track_meta(id: &str, body: &Value) -> Resp {
    let Some(path) = safe_track_path(id).filter(|f| f.is_file()) else { return not_found() };
    let meta = {
        let _g = lock();
        let mut meta = load_meta();
        if body["reset"].as_bool().unwrap_or(false) || body["reset"].as_i64().is_some_and(|n| n != 0) {
            meta.as_object_mut().unwrap().remove(id);
        } else {
            let title = clip(body["title"].as_str().unwrap_or("").trim(), 200);
            let artist = clip(body["artist"].as_str().unwrap_or("").trim(), 200);
            if title.is_empty() {
                return err(400, "Title required");
            }
            let album = clip(body["album"].as_str().unwrap_or("").trim(), 200);
            meta[id] = json!({"title": title, "artist": artist, "album": album, "album_checked": true});
        }
        save_meta(&meta);
        meta
    };
    ok(track_to_dict(&path, &load_stats(), &load_loudness(), &meta))
}

fn batch_meta(body: &Value) -> Resp {
    let Some(items) = body["items"].as_array().filter(|i| !i.is_empty()) else { return err(400, "items required") };
    let mut updated = Vec::new();
    let meta = {
        let _g = lock();
        let mut meta = load_meta();
        for item in items.iter().filter(|i| i.is_object()) {
            let path = item["id"].as_str().and_then(safe_track_path).filter(|f| f.is_file());
            let title = clip(item["title"].as_str().unwrap_or("").trim(), 200);
            let Some(path) = path.filter(|_| !title.is_empty()) else { continue };
            meta[file_name(&path)] = json!({"title": title, "artist": clip(item["artist"].as_str().unwrap_or("").trim(), 200)});
            updated.push(path);
        }
        save_meta(&meta);
        meta
    };
    let (stats, loudness) = (load_stats(), load_loudness());
    ok(json!({"tracks": updated.iter().map(|t| track_to_dict(t, &stats, &loudness, &meta)).collect::<Vec<_>>()}))
}

/// Deletes every song file (and its cover) from the library, and clears
/// everything that referred to them. Playlists themselves are kept, empty.
fn delete_all_tracks() -> Resp {
    let mut deleted = 0;
    for path in find_track_paths() {
        if fs::remove_file(&path).is_ok() {
            deleted += 1;
            let _ = fs::remove_file(cover_path_for(&path));
        }
    }
    {
        let _g = lock();
        let mut playlists = load_playlists();
        for pl in playlists.as_array_mut().unwrap() {
            pl["track_ids"] = json!([]);
        }
        save_playlists(&playlists);
        let empty_obj = json!({});
        save_stats(&empty_obj);
        let _ = store::write_json(&paths().likes, &json!([]), true, true);
        clear_history();
        let _ = store::write_json(&paths().sources, &empty_obj, true, true);
        let _ = store::write_json(&paths().loudness, &empty_obj, true, true);
        save_meta(&empty_obj);
    }
    let _ = fs::remove_dir_all(&paths().lyrics);
    ok(json!({"deleted": deleted}))
}

fn track_lyrics(id: &str, refresh: bool) -> Resp {
    let Some(path) = safe_track_path(id).filter(|f| f.is_file()) else { return not_found() };
    if !lyrics_enabled() {
        return ok(json!({"status": "disabled"}));
    }
    match lyrics::lyrics_for(&path, refresh) {
        Ok(v) => ok(v),
        // Offline or the service is down: report it, but don't cache the failure.
        Err(_) => ok(json!({"status": "error", "message": "Couldn't reach the lyrics service."})),
    }
}

/// Saves a per-song timing nudge ({offset}: seconds added to every timed line).
fn lyrics_offset(id: &str, body: &Value) -> Resp {
    if !safe_track_path(id).is_some_and(|f| f.is_file()) {
        return not_found();
    }
    let offset = body["offset"]
        .as_f64()
        .or_else(|| body["offset"].as_str().and_then(|s| s.trim().parse().ok()))
        .filter(|o: &f64| o.is_finite());
    let Some(offset) = offset else { return err(400, "offset required") };
    let offset = (offset.clamp(-30.0, 30.0) * 100.0).round() / 100.0;
    match lyrics::save_offset(id, offset) {
        Ok(()) => ok(json!({"offset": offset})),
        Err(e) => err(404, &e),
    }
}

fn update_settings(body: &Value) -> Resp {
    let _g = lock();
    let mut s = load_settings();
    for key in ["normalize_volume", "lyrics_enabled"] {
        if let Some(v) = body[key].as_bool() {
            s[key] = json!(v);
        }
    }
    if let Some(m) = body["transition_mode"].as_str().filter(|m| TRANSITION_MODES.contains(m)) {
        s["transition_mode"] = json!(m);
    }
    let number = |key: &str| body[key].as_f64().or_else(|| body[key].as_str().and_then(|x| x.trim().parse().ok()));
    if let Some(sec) = number("crossfade_seconds").filter(|v| (1.0..=12.0).contains(v)) {
        s["crossfade_seconds"] = json!(sec);
    }
    if let Some(url) = body["spotify_sync_url"].as_str() {
        let url = url.trim();
        if url.is_empty() || (is_spotify_playlist_url(url) && url.contains("open.spotify.com")) {
            s["spotify_sync_url"] = json!(url);
            tools::wake_sync();
        }
    }
    save_settings(&s);
    ok(s)
}

// ---- playlists ----

fn playlist_cover_path(pid: &str) -> PathBuf {
    paths().playlist_covers.join(format!("{pid}.jpg"))
}

/// A playlist as the client sees it: with its description and, when it has one,
/// the URL of its custom cover (the version in the URL busts caches).
fn playlist_public(pl: &Value) -> Value {
    let mut out = pl.clone();
    out["description"] = json!(pl["description"].as_str().unwrap_or(""));
    let cover = playlist_cover_path(pl["id"].as_str().unwrap_or(""));
    out["cover"] = if cover.is_file() {
        json!(format!("/playlist-covers/{}?v={}", file_name(&cover), mtime(&cover).unwrap_or(0.0) as i64))
    } else {
        Value::Null
    };
    out
}

fn reorder_playlists(body: &Value) -> Resp {
    let _g = lock();
    let playlists = load_playlists();
    let mut current: Vec<String> =
        playlists.as_array().unwrap().iter().filter_map(|p| p["id"].as_str().map(String::from)).collect();
    let ids = str_list(&body["ids"]).unwrap_or_default();
    let mut sorted = ids.clone();
    sorted.sort();
    current.sort();
    if body["ids"].as_array().is_none_or(|a| a.len() != ids.len()) || sorted != current {
        return err(400, "ids must list every playlist exactly once");
    }
    let ordered: Vec<Value> = ids
        .iter()
        .filter_map(|id| playlists.as_array().unwrap().iter().find(|p| p["id"] == *id).cloned())
        .collect();
    save_playlists(&Value::Array(ordered));
    empty(204)
}

fn update_playlist(pid: &str, body: &Value) -> Resp {
    if body.get("name").is_none() && body.get("description").is_none() {
        return err(400, "Name required");
    }
    let name = body.get("name").map(|n| clip(n.as_str().map(str::to_string).unwrap_or_else(|| n.to_string()).trim(), 120));
    if name.as_deref() == Some("") {
        return err(400, "Name required");
    }
    let _g = lock();
    let mut playlists = load_playlists();
    let Some(pl) = find_playlist(&mut playlists, pid) else { return not_found() };
    if let Some(n) = name {
        pl["name"] = json!(n);
    }
    if let Some(d) = body.get("description") {
        let d = d.as_str().map(str::to_string).unwrap_or_else(|| d.to_string());
        pl["description"] = json!(clip(d.trim(), 500));
    }
    let out = playlist_public(pl);
    save_playlists(&playlists);
    ok(out)
}

/// Sets a custom cover from the request body (a JPEG - the client crops and
/// shrinks whatever the user picked before sending it).
fn set_playlist_cover(pid: &str, data: &[u8]) -> Resp {
    if !data.starts_with(&[0xff, 0xd8, 0xff]) {
        return err(400, "A JPEG image is required");
    }
    if data.len() > 3_000_000 {
        return err(413, "Image too large");
    }
    let _g = lock();
    let mut playlists = load_playlists();
    let Some(pl) = find_playlist(&mut playlists, pid) else { return not_found() };
    let _ = fs::create_dir_all(&paths().playlist_covers);
    if let Err(e) = fs::write(playlist_cover_path(pid), data) {
        return err(500, &e.to_string());
    }
    ok(playlist_public(pl))
}

/// Applies `f` to one playlist under the lock, saves it, and returns it.
fn edit_playlist(pid: &str, f: impl FnOnce(&mut Value) -> Result<(), (u16, &'static str)>) -> Resp {
    let _g = lock();
    let mut playlists = load_playlists();
    let Some(pl) = find_playlist(&mut playlists, pid) else { return not_found() };
    if let Err((status, msg)) = f(pl) {
        return err(status, msg);
    }
    let out = pl.clone();
    save_playlists(&playlists);
    ok(out)
}

// ---------------- server ----------------

fn handle(mut req: Request) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let (raw_path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = unquote(raw_path);
    let range = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Range"))
        .map(|h| h.value.as_str().to_string());
    let if_none_match = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("If-None-Match"))
        .map(|h| h.value.as_str().to_string());
    // A song being added from the user's computer can be large: stream it to disk instead of
    // reading it into memory like the small JSON bodies below.
    if method == Method::Post && path == "/api/upload" {
        let name = unquote(query_param(query, "name").unwrap_or(""));
        let resp = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::library::import::receive(&name, req.as_reader()))) {
            Ok(Ok(v)) => ok(v),
            Ok(Err(e)) => err(400, &e),
            Err(_) => err(500, "Internal error"),
        };
        let _ = req.respond(resp);
        return;
    }
    let mut body = Vec::new();
    if method != Method::Get && method != Method::Head {
        let _ = req.as_reader().take(10_000_000).read_to_end(&mut body);
    }
    let cx = Ctx { body: &body, query, range: range.as_deref(), if_none_match: if_none_match.as_deref() };
    let resp = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| route(method, &path, &cx)))
        .unwrap_or_else(|_| {
            store::error(format!("Handler panicked for {path}"));
            err(500, "Internal error")
        });
    let _ = req.respond(resp);
}

/// Serves forever on 127.0.0.1:`port` (blocks).
pub fn serve(port: u16) -> Result<(), String> {
    let server = Server::http(("127.0.0.1", port)).map_err(|e| format!("Couldn't listen on port {port}: {e}"))?;
    for req in server.incoming_requests() {
        std::thread::spawn(move || handle(req));
    }
    Ok(())
}
