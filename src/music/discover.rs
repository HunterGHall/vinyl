//! Music discovery through Deezer's public API (no key needed): artists related to
//! one you like, an artist's top songs, genres and genre charts. Songs come back
//! in the same shape as catalog search results, so they can be downloaded through
//! the normal catalog job, and carry a 30 s `preview` URL for instant listening.

use crate::music::download;
use crate::base::util;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

const API: &str = "https://api.deezer.com";
// Preview URLs are signed and expire after about an hour, so keep results fresher than that.
const TTL: Duration = Duration::from_secs(20 * 60);

static CACHE: LazyLock<Mutex<HashMap<String, (Instant, Value)>>> = LazyLock::new(Default::default);

/// Runs `f` unless a fresh result is cached; only complete results (`.1 == true`) are cached.
fn cached(key: &str, f: impl FnOnce() -> Result<(Value, bool), String>) -> Result<Value, String> {
    if let Some((at, v)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).get(key) {
        if at.elapsed() < TTL {
            return Ok(v.clone());
        }
    }
    let (v, complete) = f()?;
    if complete {
        CACHE.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), (Instant::now(), v.clone()));
    }
    Ok(v)
}

/// Deezer allows about 50 requests per 5 s, so space calls out and retry once on a quota error.
static LAST_CALL: LazyLock<Mutex<Instant>> = LazyLock::new(|| Mutex::new(Instant::now()));
fn throttle() {
    let mut last = LAST_CALL.lock().unwrap_or_else(|e| e.into_inner());
    let gap = Duration::from_millis(120);
    if let Some(wait) = gap.checked_sub(last.elapsed()) {
        std::thread::sleep(wait);
    }
    *last = Instant::now();
}

fn get(path: &str) -> Result<Value, String> {
    for attempt in 0..3 {
        throttle();
        let text = util::http_get_text(&format!("{API}{path}"), 12, 1)
            .map_err(|_| "Couldn't reach the music catalog. Check your connection and try again.".to_string())?;
        let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if v["error"].is_object() {
            if v["error"]["code"] == 4 && attempt < 2 {
                std::thread::sleep(Duration::from_millis(1200));
                continue;
            }
            return Err(v["error"]["message"].as_str().unwrap_or("Catalog error").to_string());
        }
        return Ok(v);
    }
    Err("Catalog is busy, try again".into())
}

fn artist_json(a: &Value) -> Value {
    json!({
        "id": a["id"],
        "name": a["name"],
        "picture": a["picture_medium"],
        "fans": a["nb_fan"],
    })
}

fn track_item(t: &Value, because: &str) -> Option<Value> {
    let id = t["id"].as_i64()?;
    let title = t["title"].as_str()?;
    let artist = t["artist"]["name"].as_str().unwrap_or("");
    let key = format!("deezer:{id}");
    let album = &t["album"];
    let cover_xl = album["cover_xl"].as_str().or_else(|| album["cover_big"].as_str()).unwrap_or("");
    let thumb = album["cover_medium"].as_str().unwrap_or(cover_xl);
    let duration = t["duration"].as_f64().unwrap_or(0.0);
    Some(json!({
        "kind": "catalog",
        "id": key,
        "title": title,
        "artist": artist,
        "artist_id": t["artist"]["id"],
        "album": album["title"],
        "duration": duration,
        "thumbnail": thumb,
        "preview": t["preview"],
        "because": because,
        "in_library": download::catalog_in_library(&key, artist, title),
        "catalog": {"id": key, "title": title, "artist": artist, "album": album["title"], "duration": duration, "cover": cover_xl},
    }))
}

fn items_of(list: &Value, because: &str) -> Vec<Value> {
    list["data"].as_array().into_iter().flatten().filter_map(|t| track_item(t, because)).collect()
}

fn resolve_artist(name: &str) -> Result<Value, String> {
    let v = get(&format!("/search/artist?limit=5&q={}", util::quote_all(name)))?;
    let list = v["data"].as_array().cloned().unwrap_or_default();
    let lower = name.to_lowercase();
    list.iter()
        .find(|a| a["name"].as_str().is_some_and(|n| n.to_lowercase() == lower))
        .or_else(|| list.first())
        .cloned()
        .ok_or_else(|| format!("No artist named {name}"))
}

/// Artists related to `name` (as Deezer sees them), each with a few top songs.
pub fn similar(name: &str, n: usize, per: usize) -> Result<Value, String> {
    let (n, per) = (n.clamp(1, 12), per.clamp(1, 6));
    cached(&format!("similar:{}:{n}:{per}", name.to_lowercase()), || {
        let artist = resolve_artist(name)?;
        let id = artist["id"].as_i64().ok_or("Bad artist")?;
        let related = get(&format!("/artist/{id}/related?limit={n}"))?;
        let rel: Vec<Value> = related["data"].as_array().cloned().unwrap_or_default();
        let mut out: Vec<Value> = vec![Value::Null; rel.len()];
        let failed = std::sync::atomic::AtomicBool::new(false);
        let failed = &failed;
        std::thread::scope(|s| {
            for (slot, a) in out.iter_mut().zip(rel.iter()) {
                s.spawn(move || {
                    let Some(rid) = a["id"].as_i64() else { return };
                    let because = name.to_string();
                    let tracks = match get(&format!("/artist/{rid}/top?limit={}", per + 3)) {
                        Ok(v) => items_of(&v, &because),
                        Err(_) => {
                            failed.store(true, std::sync::atomic::Ordering::Relaxed);
                            Vec::new()
                        }
                    };
                    let mut j = artist_json(a);
                    j["tracks"] = Value::Array(tracks.into_iter().filter(|t| t["in_library"] == json!(false)).take(per).collect());
                    *slot = j;
                });
            }
        });
        let related: Vec<Value> = out.into_iter().filter(|v| v.is_object()).collect();
        let complete = !failed.load(std::sync::atomic::Ordering::Relaxed);
        Ok((json!({"artist": artist_json(&artist), "related": related, "complete": complete}), complete))
    })
}

/// One artist: top songs plus related artists.
pub fn artist(id: i64) -> Result<Value, String> {
    cached(&format!("artist:{id}"), || {
        let info = get(&format!("/artist/{id}"))?;
        let name = info["name"].as_str().unwrap_or("").to_string();
        let top = get(&format!("/artist/{id}/top?limit=20"))?;
        let related = get(&format!("/artist/{id}/related?limit=12")).unwrap_or(Value::Null);
        Ok((json!({
            "artist": artist_json(&info),
            "tracks": items_of(&top, &name),
            "related": related["data"].as_array().into_iter().flatten().map(artist_json).collect::<Vec<_>>(),
        }), true))
    })
}

pub fn genres() -> Result<Value, String> {
    cached("genres", || {
        let v = get("/genre")?;
        let list: Vec<Value> = v["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|g| g["id"].as_i64().is_some_and(|i| i != 0))
            .map(|g| json!({"id": g["id"], "name": g["name"], "picture": g["picture_medium"]}))
            .collect();
        Ok((json!({"genres": list}), true))
    })
}

/// A genre's chart songs and popular artists.
pub fn genre(id: i64) -> Result<Value, String> {
    cached(&format!("genre:{id}"), || {
        let tracks = get(&format!("/chart/{id}/tracks?limit=40"))?;
        let artists = get(&format!("/genre/{id}/artists")).unwrap_or(Value::Null);
        Ok((json!({
            "tracks": items_of(&tracks, ""),
            "artists": artists["data"].as_array().into_iter().flatten().take(12).map(artist_json).collect::<Vec<_>>(),
        }), true))
    })
}
