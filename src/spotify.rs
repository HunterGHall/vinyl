//! Reading Spotify links without credentials: oEmbed plus the public embed page.

use crate::store;
use crate::util::{http_get_text, quote_all};
use regex_lite::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

static NEXT_DATA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)<script[^>]*id="__NEXT_DATA__"[^>]*>(.*?)</script>"#).unwrap());
static TRACK_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"open\.spotify\.com/(?:intl-[a-z]+/)?track/([A-Za-z0-9]+)").unwrap());
static PLAYLIST_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"open\.spotify\.com/(?:intl-[a-z]+/)?playlist/([A-Za-z0-9]+)").unwrap());
static ARTIST_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""artists":\[\{"name":"([^"]+)""#).unwrap());
static TRACK_URI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""uri":"spotify:track:([A-Za-z0-9]+)""#).unwrap());

fn oembed(url: &str) -> Result<Value, String> {
    let text = http_get_text(&format!("https://open.spotify.com/oembed?url={}", quote_all(url)), 15, 3)?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// (raw html, parsed page data or None) of a Spotify embed page. The page
/// carries its data as JSON in a script tag; reading that is sturdier than
/// pattern-matching the markup, with the patterns kept as the fallback.
fn embed_data(kind: &str, id: &str) -> Result<(String, Option<Value>), String> {
    let html = http_get_text(&format!("https://open.spotify.com/embed/{kind}/{id}"), 15, 3)?;
    let data = NEXT_DATA_RE.captures(&html).and_then(|c| match serde_json::from_str(&c[1]) {
        Ok(v) => Some(v),
        Err(_) => {
            store::warn("Spotify embed page had unreadable JSON");
            None
        }
    });
    Ok((html, data))
}

/// Every object nested anywhere in `node`, in document order.
fn walk<'a>(node: &'a Value, out: &mut Vec<&'a Map<String, Value>>) {
    match node {
        Value::Object(m) => {
            out.push(m);
            m.values().for_each(|v| walk(v, out));
        }
        Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
        _ => {}
    }
}

fn objects(page: &Option<Value>) -> Vec<&Map<String, Value>> {
    let mut out = Vec::new();
    if let Some(p) = page {
        walk(p, &mut out);
    }
    out
}

/// (title, artist, cover_url) for a Spotify track link, via the public oEmbed
/// endpoint plus a light read of the embed page for the artist name.
pub fn fetch_track_metadata(url: &str) -> Result<(String, String, Option<String>), String> {
    let data = oembed(url)?;
    let title = data["title"].as_str().unwrap_or("").to_string();
    let cover = data["thumbnail_url"].as_str().map(String::from);

    let mut artist = String::new();
    if let Some(c) = TRACK_ID_RE.captures(url) {
        match embed_data("track", &c[1]) {
            Ok((html, page)) => {
                for d in objects(&page) {
                    if let Some(name) = d
                        .get("artists")
                        .and_then(|a| a.as_array())
                        .and_then(|a| a.first())
                        .and_then(|a| a["name"].as_str())
                        .filter(|n| !n.is_empty())
                    {
                        artist = name.to_string();
                        break;
                    }
                    if let Some(sub) = d.get("subtitle").and_then(|s| s.as_str()) {
                        if d.get("uri").and_then(|u| u.as_str()).is_some_and(|u| u.starts_with("spotify:track:")) {
                            artist = sub.to_string();
                            break;
                        }
                    }
                }
                if artist.is_empty() {
                    if let Some(m) = ARTIST_RE.captures(&html) {
                        artist = m[1].to_string();
                    }
                }
                if artist.is_empty() {
                    store::warn("Spotify track page had no artist - its layout may have changed");
                }
            }
            Err(e) => store::warn(format!("Couldn't read the artist for {url}: {e}")),
        }
    }
    Ok((title, artist, cover))
}

/// (playlist_title, [track_url, ...]) for a Spotify playlist link. oEmbed gives
/// the playlist's own title; the track URLs come from the embed page's data
/// (undocumented, so read two independent ways). The embed page only lists the
/// first ~100 tracks of a very long playlist.
pub fn fetch_playlist_tracks(url: &str) -> Result<(String, Vec<String>), String> {
    let data = oembed(url)?;
    let title = data["title"].as_str().unwrap_or("").to_string();

    let mut track_urls = Vec::new();
    if let Some(c) = PLAYLIST_ID_RE.captures(url) {
        let (html, page) = embed_data("playlist", &c[1])?;
        let mut ids: Vec<String> = objects(&page)
            .iter()
            .filter_map(|d| d.get("uri")?.as_str()?.strip_prefix("spotify:track:").map(String::from))
            .collect();
        if ids.is_empty() {
            ids = TRACK_URI_RE.captures_iter(&html).map(|c| c[1].to_string()).collect();
        }
        if ids.is_empty() {
            store::warn("Spotify playlist page had no tracks - its layout may have changed");
        }
        let mut seen = std::collections::HashSet::new();
        track_urls = ids
            .into_iter()
            .filter(|i| seen.insert(i.clone()))
            .map(|i| format!("https://open.spotify.com/track/{i}"))
            .collect();
    }
    Ok((title, track_urls))
}
