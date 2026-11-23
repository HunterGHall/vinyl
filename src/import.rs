//! Adding songs from files on this computer: the page streams each file here, and it is tidied
//! into the library like a downloaded song: named "Artist - Title", with its cover art (taken
//! from the file's own tags when it has one) and album name.

use crate::library::{self, AUDIO_EXTS, file_name};
use crate::store::{self, paths};
use crate::util::{self, sanitize_filename};
use serde_json::{Value, json};
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_BYTES: u64 = 1_500_000_000; // per file

#[derive(Default)]
struct Tags {
    title: String,
    artist: String,
    album: String,
}

/// Title, artist and album from the file's own tags (ID3, Vorbis comments, MP4 atoms...), if any.
fn read_tags(path: &Path) -> Tags {
    let out = util::command("ffprobe")
        .args(["-v", "quiet", "-print_format", "json", "-show_entries", "format_tags"])
        .arg(path)
        .output();
    let Ok(o) = out else { return Tags::default() };
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
    let tags = v["format"]["tags"].as_object().cloned().unwrap_or_default();
    let get = |name: &str| -> String {
        tags.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).and_then(|(_, v)| v.as_str()).unwrap_or("").trim().to_string()
    };
    let artist = [get("artist"), get("album_artist")].into_iter().find(|a| !a.is_empty()).unwrap_or_default();
    Tags { title: get("title"), artist, album: get("album") }
}

/// Saves the picture embedded in the file (if any) as the song's cover: a 300 px square, like the
/// covers of downloaded songs.
fn extract_cover(audio: &Path, cover: &Path) {
    let _ = std::fs::create_dir_all(&paths().covers);
    let ok = util::command("ffmpeg")
        .args(["-y", "-v", "quiet", "-i"])
        .arg(audio)
        .args(["-an", "-frames:v", "1", "-vf", "scale=300:300:force_original_aspect_ratio=increase,crop=300:300"])
        .arg(cover)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        let _ = std::fs::remove_file(cover);
    }
}

/// Streams one uploaded song into the library. `name` is the file's name on the user's computer.
pub fn receive(name: &str, reader: &mut dyn Read) -> Result<Value, String> {
    // only the file's own name, whatever folders the browser mentioned
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let path = Path::new(base);
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !AUDIO_EXTS.contains(&ext.as_str()) {
        return Err(format!("{base} isn't a supported audio file ({}).", AUDIO_EXTS.join(", ")));
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

    // 1. write it to a scratch folder inside the library (not scanned as songs) under its real extension
    let incoming = paths().library.join(".incoming");
    std::fs::create_dir_all(&incoming).map_err(|e| e.to_string())?;
    let tmp = incoming.join(format!("{}.{ext}", &uuid::Uuid::new_v4().simple().to_string()[..12]));
    let result = (|| -> Result<Value, String> {
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let copied = std::io::copy(&mut reader.take(MAX_BYTES + 1), &mut file).map_err(|e| format!("Upload failed: {e}"))?;
        drop(file);
        if copied > MAX_BYTES {
            return Err("That file is too large.".into());
        }
        if copied == 0 {
            return Err("That file is empty.".into());
        }

        // 2. name it "Artist - Title" from its tags; a file already called that way keeps its name
        let tags = read_tags(&tmp);
        let (artist, title) = match stem.split_once(" - ") {
            Some((a, t)) if tags.title.is_empty() => (a.trim().to_string(), t.trim().to_string()),
            _ => (tags.artist.clone(), if tags.title.is_empty() { stem.trim().to_string() } else { tags.title.clone() }),
        };
        let display = if artist.is_empty() { title.clone() } else { format!("{artist} - {title}") };
        let final_stem: String = sanitize_filename(&display).chars().take(150).collect();
        let final_path: PathBuf = paths().library.join(format!("{final_stem}.{ext}"));
        let id = file_name(&final_path);
        if final_path.exists() {
            return Ok(json!({"id": id, "title": title, "artist": artist, "duplicate": true}));
        }
        std::fs::rename(&tmp, &final_path)
            .or_else(|_| std::fs::copy(&tmp, &final_path).map(|_| ()))
            .map_err(|e| e.to_string())?;

        // 3. cover, album, and the usual follow-ups (lyrics, loudness, album lookup)
        extract_cover(&final_path, &library::cover_path_for(&final_path));
        library::remember_real_names(&final_path, &title, &artist);
        library::remember_album(&final_path, &tags.album);
        library::scan_loudness_async();
        library::backfill_albums_async();
        crate::lyrics::prefetch_async(vec![id.clone()]);
        Ok(json!({"id": id, "title": title, "artist": artist, "album": tags.album, "duplicate": false}))
    })();
    let _ = std::fs::remove_file(&tmp);
    if let Err(e) = &result {
        store::warn(format!("Couldn't add {base}: {e}"));
    }
    result
}
