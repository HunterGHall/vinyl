//! Lyrics, looked up on LRCLIB by artist/title/length and cached under
//! `data/lyrics`, so each song is fetched at most once (a miss is retried after
//! a week, or as soon as the song's name changes).

use crate::library::{self, file_name, name_fields, track_duration};
use crate::base::store::{self, lock, paths};
use crate::base::util::{quote_all, words};
use regex_lite::Regex;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

const LRCLIB_URL: &str = "https://lrclib.net/api/search";
const MISS_RETRY_SECONDS: f64 = 7.0 * 24.0 * 3600.0;

// How far a database entry's length may differ from the file before its timing
// stops being trusted (timed lyrics from another edit drift out of sync):
const EXACT_SECONDS: f64 = 2.0; // trusted as is
const APPROX_SECONDS: f64 = 6.0; // timed, but flagged - the user can nudge the timing
const TEXT_ONLY_SECONDS: f64 = 12.0; // same song, other edit: keep the words, drop the timing

static BRACKETS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[\(\[][^\)\]]*[\)\]]").unwrap());
static SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s+-\s+(?:remaster|radio edit|single|from\b|live\b|mono\b|stereo\b|version\b).*$").unwrap()
});
static SPACES_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static FIRST_ARTIST_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\s*(?:,|&|\bfeat\.?|\bft\.?|\bwith\b)\s*").unwrap());
static STAMP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[(\d+):(\d+(?:\.\d+)?)\]").unwrap());
static LEADING_TAGS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:\[[^\]]*\])+").unwrap());

pub fn cache_path(track_id: &str) -> PathBuf {
    let digest = Sha1::digest(track_id.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    paths().lyrics.join(format!("{hex}.json"))
}

/// Drops "(feat. X)", "[Official Video]", "- Remastered 2011" and similar,
/// which often stop a title matching the database entry.
pub fn clean_title(title: &str) -> String {
    let t = BRACKETS_RE.replace_all(title, "");
    let t = SUFFIX_RE.replace(&t, "");
    let t = SPACES_RE.replace_all(&t, " ");
    let t = t.trim().trim_matches(|c| c == ' ' || c == '-').to_string();
    if t.is_empty() { title.to_string() } else { t }
}

pub fn first_artist(artist: &str) -> String {
    FIRST_ARTIST_RE.splitn(artist, 2).next().unwrap_or("").trim().to_string()
}

/// `[mm:ss.xx]` timed lines -> `[{"t": seconds, "text": line}]`, sorted.
fn parse_lrc(text: &str) -> Vec<Value> {
    let mut lines: Vec<(f64, String)> = Vec::new();
    for raw in text.lines() {
        let stamps: Vec<_> = STAMP_RE.captures_iter(raw).collect();
        if stamps.is_empty() {
            continue;
        }
        let content = LEADING_TAGS_RE.replace(raw, "").trim().to_string();
        for s in stamps {
            let t = s[1].parse::<f64>().unwrap_or(0.0) * 60.0 + s[2].parse::<f64>().unwrap_or(0.0);
            lines.push(((t * 100.0).round() / 100.0, content.clone()));
        }
    }
    lines.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    lines.into_iter().map(|(t, text)| json!({"t": t, "text": text})).collect()
}

fn search(params: &[(&str, &str)]) -> Result<Vec<Value>, String> {
    let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", quote_all(v))).collect();
    let url = format!("{LRCLIB_URL}?{}", query.join("&"));
    let resp = crate::base::util::agent().get(&url)
        .set("User-Agent", "music-player-local/1.0 (personal project)")
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| e.to_string())?;
    let text = resp.into_string().map_err(|e| e.to_string())?;
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(v.as_array().cloned().unwrap_or_default())
}

fn dur(r: &Value) -> f64 {
    r["duration"].as_f64().unwrap_or(0.0)
}

fn has_text(r: &Value, key: &str) -> bool {
    r[key].as_str().is_some_and(|s| !s.is_empty())
}

#[derive(Clone)]
struct Pick {
    entry: Value,
    diff: Option<f64>,
    tier: &'static str,
}

fn tier_rank(tier: &str) -> u8 {
    match tier {
        "exact" => 0,
        "approximate" => 1,
        _ => 2,
    }
}

/// The best database entry for the song: same title and artist and a similar
/// length (to avoid live/remix versions), preferring timed lyrics.
fn pick(results: &[Value], titles: &[String], artists: &[String], duration: f64) -> Option<Pick> {
    let title_words: Vec<String> = titles.iter().map(|t| words(t)).collect();
    let artist_words: Vec<String> = artists.iter().filter(|a| !a.is_empty()).map(|a| words(a)).collect();
    let usable: Vec<&Value> = results
        .iter()
        .filter(|r| {
            (has_text(r, "syncedLyrics") || has_text(r, "plainLyrics") || r["instrumental"].as_bool().unwrap_or(false))
                && title_words.contains(&words(r["trackName"].as_str().unwrap_or("")))
                && (artist_words.is_empty() || {
                    let ra = words(r["artistName"].as_str().unwrap_or(""));
                    artist_words.iter().any(|w| !w.is_empty() && (ra.contains(w.as_str()) || w.contains(ra.as_str())))
                })
        })
        .collect();
    if duration <= 0.0 {
        return usable
            .iter()
            .min_by_key(|r| !has_text(r, "syncedLyrics"))
            .map(|r| Pick { entry: (*r).clone(), diff: None, tier: "exact" });
    }
    for (tol, tier) in [(EXACT_SECONDS, "exact"), (APPROX_SECONDS, "approximate"), (TEXT_ONLY_SECONDS, "text")] {
        let best = usable
            .iter()
            .filter(|r| (dur(r) - duration).abs() <= tol)
            .min_by(|a, b| {
                (!has_text(a, "syncedLyrics"), (dur(a) - duration).abs())
                    .partial_cmp(&(!has_text(b, "syncedLyrics"), (dur(b) - duration).abs()))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if let Some(best) = best {
            return Some(Pick {
                entry: (*best).clone(),
                diff: Some(((duration - dur(best)) * 10.0).round() / 10.0),
                tier,
            });
        }
    }
    None
}

/// Looks the song up, retrying with a cleaned-up title and the first credited
/// artist. Errors only on network failures.
fn fetch(title: &str, artist: &str, duration: f64) -> Result<Value, String> {
    let clean = clean_title(title);
    let first = first_artist(artist);
    let titles = vec![title.to_string(), clean.clone()];
    let artists = vec![artist.to_string(), first.clone()];
    let mut attempts: Vec<(String, String)> = Vec::new();
    for (t, a) in [(title, artist), (clean.as_str(), artist), (clean.as_str(), first.as_str())] {
        let pair = (t.to_string(), a.to_string());
        if !attempts.contains(&pair) {
            attempts.push(pair);
        }
    }
    let mut best: Option<Pick> = None;
    let mut searches: Vec<Vec<(&str, String)>> = attempts
        .iter()
        .map(|(t, a)| {
            let mut p = vec![("track_name", t.clone())];
            if !a.is_empty() {
                p.push(("artist_name", a.clone()));
            }
            p
        })
        .collect();
    searches.push(vec![("q", format!("{first} {clean}").trim().to_string())]);
    for params in &searches {
        let p: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let candidate = pick(&search(&p)?, &titles, &artists, duration);
        // keep the closest-length match across the different searches
        if let Some(c) = candidate {
            let better = match &best {
                None => true,
                Some(b) => (tier_rank(c.tier), c.diff.unwrap_or(0.0).abs()) < (tier_rank(b.tier), b.diff.unwrap_or(0.0).abs()),
            };
            if better {
                best = Some(c);
            }
        }
        if best.as_ref().is_some_and(|b| b.tier == "exact") {
            break;
        }
    }
    let Some(Pick { entry, diff, tier }) = best else {
        return Ok(json!({"status": "none"}));
    };
    if entry["instrumental"].as_bool().unwrap_or(false) && !has_text(&entry, "plainLyrics") && !has_text(&entry, "syncedLyrics") {
        return Ok(json!({"status": "instrumental"}));
    }
    let timed = has_text(&entry, "syncedLyrics").then(|| parse_lrc(entry["syncedLyrics"].as_str().unwrap()));
    let plain = if has_text(&entry, "plainLyrics") {
        Some(entry["plainLyrics"].as_str().unwrap().to_string())
    } else {
        timed.as_ref().filter(|t| !t.is_empty()).map(|t| {
            t.iter().map(|l| l["text"].as_str().unwrap_or("")).collect::<Vec<_>>().join("\n")
        })
    };
    Ok(json!({
        "status": "found",
        // a different edit of the song has different timing, so only the words are kept
        "synced": if tier != "text" { timed.map(Value::from).unwrap_or(Value::Null) } else { Value::Null },
        "plain": plain,
        "timing": tier,
        "duration_diff": diff,   // file length minus the database entry's length, in seconds
        "match": {"title": entry["trackName"], "artist": entry["artistName"], "duration": entry["duration"]},
    }))
}

fn with_offset(mut result: Value, offset: Value) -> Value {
    result["offset"] = offset;
    result
}

/// The lyrics result for a song: from the cache when it's still good, otherwise
/// looked up (and cached). Network errors are returned, never cached.
pub fn lyrics_for(path: &Path, refresh: bool) -> Result<Value, String> {
    let (title, artist) = name_fields(path, &library::load_meta());
    let query = json!({"title": title, "artist": artist});
    let cache_file = cache_path(&file_name(path));

    if cache_file.is_file() && !refresh {
        let cached = std::fs::read_to_string(&cache_file)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()));
        match cached {
            Ok(cached) if cached["result"].is_object() => {
                let result = &cached["result"];
                let status = result["status"].as_str().unwrap_or("");
                let expired = status == "none"
                    && store::now() - cached["fetched"].as_f64().unwrap_or(0.0) > MISS_RETRY_SECONDS;
                // Entries saved before lengths were checked strictly: look them up
                // again if the cached version's length is clearly not this file's.
                let matched = result["match"]["duration"].as_f64().filter(|d| *d != 0.0);
                let unchecked = status == "found"
                    && result.get("timing").is_none()
                    && matched.is_some_and(|m| (track_duration(path) - m).abs() > EXACT_SECONDS);
                if cached["query"] == query && !expired && !unchecked {
                    let offset = cached.get("offset").cloned().unwrap_or(json!(0));
                    return Ok(with_offset(result.clone(), offset));
                }
            }
            Ok(_) => {}
            Err(e) => store::warn(format!("Unreadable lyrics cache for {} ({e}); looking it up again", file_name(path))),
        }
    }

    let result = fetch(&title, &artist, track_duration(path))?;
    let _g = lock();
    // a fresh lookup may be a different edit, so any saved timing nudge starts over
    let entry = json!({"query": query, "fetched": store::now(), "offset": 0, "result": result});
    if let Err(e) = store::write_json(&cache_file, &entry, false, false) {
        store::warn(format!("Couldn't cache lyrics: {e}"));
    }
    Ok(with_offset(entry["result"].clone(), json!(0)))
}

/// Saves a per-song timing nudge. Err(msg) when there are no saved lyrics.
pub fn save_offset(track_id: &str, offset: f64) -> Result<(), String> {
    let cache_file = cache_path(track_id);
    let _g = lock();
    let mut cached: Value = std::fs::read_to_string(&cache_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .ok_or("No lyrics saved for this song")?;
    cached["offset"] = json!(offset);
    store::write_json(&cache_file, &cached, false, false).map_err(|e| e.to_string())
}

/// Fetches (and caches) a freshly downloaded song's lyrics as part of its
/// download, so they're there - offline included - from the first play.
/// Never fails: a failure just means the lyrics load on demand later.
pub fn fetch_with_song(path: &Path) {
    if !library::lyrics_enabled() {
        return;
    }
    if let Err(e) = lyrics_for(path, false) {
        store::info(format!("Lyrics lookup failed for {}: {e}", file_name(path)));
    }
}

/// After an import, looks the new songs' lyrics up in the background so
/// they're ready (and work offline) by the first play.
pub fn prefetch_async(track_ids: Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<String> = track_ids.into_iter().filter(|t| !t.is_empty() && seen.insert(t.clone())).collect();
    if ids.is_empty() || !library::lyrics_enabled() {
        return;
    }
    std::thread::spawn(move || {
        for id in ids {
            let Some(path) = library::safe_track_path(&id).filter(|p| p.is_file()) else { continue };
            if lyrics_for(&path, false).is_err() {
                return; // offline - stop quietly; lyrics load on demand later
            }
            std::thread::sleep(Duration::from_millis(400)); // be gentle with the free service
        }
    });
}
