"""example rewrite generated from claude"""
# uses non-existent imports


from __future__ import annotations

import copy
import json
import os
import subprocess
import sys
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Optional, TypeVar

from .. import library
from ..library import (
    add_track_to_playlist_record,
    cover_path_for,
    create_playlist_record,
    existing_for_source,
    file_name,
    file_stem,
    record_source,
    remember_real_names,
    rename_playlist_record,
    source_key,
)
from ..base import store, util
from ..base.store import paths
from ..base.util import download_image, is_spotify_playlist_url, is_spotify_url, sanitize_filename
from ..system import deps
from . import lyrics, spotify

R = TypeVar("R")


class DownloadError(Exception):
    """A user-facing failure message (the Rust code's `Err(String)`)."""


# ---------------- jobs ----------------


class Job:
    """A background job's state, shared between its worker thread and the status route."""

    def __init__(self, state: dict) -> None:
        self._state = state
        self._lock = threading.Lock()

    def with_(self, f: Callable[[dict], R]) -> R:
        with self._lock:
            return f(self._state)

    def snapshot(self) -> dict:
        return self.with_(copy.deepcopy)

    def push(self, key: str, item: Any) -> None:
        self.with_(lambda j: j[key].append(item))

    def update(self, **fields: Any) -> None:
        self.with_(lambda j: j.update(fields))


JOBS: dict[str, Job] = {}
_JOBS_LOCK = threading.Lock()


def register(job: Job) -> str:
    job_id = library.new_id()
    with _JOBS_LOCK:
        JOBS[job_id] = job
    return job_id


def lookup(job_id: str) -> Optional[Job]:
    with _JOBS_LOCK:
        return JOBS.get(job_id)


def new_download_job(kind: str) -> Job:
    return Job({
        "kind": kind,                # "track" | "playlist"
        "status": "downloading",     # downloading | done | error
        "percent": "",               # label for the file being fetched: "42.0%" / "Converting..." / ...
        "fraction": 0.0,             # 0..1 progress of the file being fetched (numeric, for the bar)
        "current_title": None,
        "title": None,               # final title, for a "track" job
        "as_playlist": True,         # False when a playlist link was imported as plain songs
        "playlist_id": None,         # for a "playlist" job (None unless as_playlist)
        "playlist_name": None,
        "total": 0,
        "duplicate": False,          # a "track" job whose track was already in the library
        "skipped": [],               # [{id, title}, ...] already in library (playlist job)
        "added": [],                 # [{id, title}, ...]
        "failed": [],                # [{title, error}, ...]
        "error": None,
    })


# ---------------- helpers ----------------


def _no_window() -> dict:
    """Keeps a console window from flashing up for each subprocess on Windows."""
    if sys.platform == "win32":
        return {"creationflags": subprocess.CREATE_NO_WINDOW}
    return {}


def _run_output(args: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(
        args,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        **_no_window(),
    )


def _same_file(a: Path, b: Path) -> bool:
    try:
        return os.path.samefile(a, b)
    except OSError:
        return False


def _remove_quietly(p: Path) -> None:
    try:
        p.unlink()
    except OSError:
        pass


def _as_float(v: Any) -> float:
    if isinstance(v, (int, float)) and not isinstance(v, bool):
        return float(v)
    return 0.0


def _as_str(v: Any) -> str:
    return v if isinstance(v, str) else ""


# ---------------- yt-dlp ----------------


@dataclass
class Progress:
    status: str
    downloaded: float
    total: float
    title: str


def num(s: str) -> float:
    try:
        return float(s.strip())
    except ValueError:
        return 0.0


def apply_progress(job: Job, p: Progress) -> None:
    def update(j: dict) -> None:
        if p.status == "downloading":
            if p.total > 0.0:
                f = min(max(p.downloaded / p.total, 0.0), 1.0)
                j["fraction"] = f
                j["percent"] = f"{f * 100.0:.1f}%"
            else:
                j["percent"] = ""  # size unknown: the page shows an indeterminate bar
            if p.title and p.title != "NA":
                j["current_title"] = p.title
        elif p.status == "finished":
            j["fraction"] = 1.0
            j["percent"] = "Converting..."

    job.with_(update)


def run_ytdlp(target: str, job: Optional[Job] = None) -> dict:
    """Runs yt-dlp on `target` (a URL or `ytsearch1:...`), downloading one video as
    mp3 into the library. Returns its info dict."""
    template = str(paths().library / "%(title)s.%(ext)s")
    args = [
        "yt-dlp",
        "--no-warnings", "--newline", "--progress", "--no-simulate", "--no-playlist",
        "-f", "bestaudio/best", "-x", "--audio-format", "mp3", "--audio-quality", "192",
        "-o", template,
    ]
    ffmpeg = deps.find("ffmpeg")
    if ffmpeg is not None:
        args += ["--ffmpeg-location", str(Path(ffmpeg).parent)]
    args += [
        "--progress-template",
        "download:PROG\t%(progress.status)s\t%(progress.downloaded_bytes)s\t%(progress.total_bytes)s"
        "\t%(progress.total_bytes_estimate)s\t%(info.title)s",
        "--print",
        "after_move:JSON:%()j",
        target,
    ]
    try:
        child = subprocess.Popen(
            args,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
            **_no_window(),
        )
    except OSError as e:
        raise DownloadError(deps.friendly(f"Couldn't run yt-dlp ({e})")) from e

    errors: list[str] = []

    def drain_stderr() -> None:
        for line in child.stderr:
            errors.append(line.rstrip("\r\n"))

    err_thread = threading.Thread(target=drain_stderr, daemon=True)
    err_thread.start()

    info: Optional[dict] = None
    for raw in child.stdout:
        line = raw.rstrip("\r\n")
        if line.startswith("JSON:"):
            try:
                info = json.loads(line[len("JSON:"):])
            except ValueError:
                pass
        elif line.startswith("PROG\t"):
            f = line[len("PROG\t"):].split("\t", 4)
            if job is not None and len(f) == 5:
                dl, total, est = num(f[1]), num(f[2]), num(f[3])
                apply_progress(job, Progress(
                    status=f[0],
                    downloaded=dl,
                    total=total if total > 0.0 else est,
                    title=f[4],
                ))
    returncode = child.wait()
    err_thread.join()

    if info is not None and returncode == 0:
        return info

    msg: Optional[str] = None
    for l in reversed(errors):
        if "ERROR" in l:
            msg = l
            while msg.startswith("ERROR: "):
                msg = msg[len("ERROR: "):]
            break
    if msg is None and errors:
        msg = errors[-1]
    if msg is not None:
        raise DownloadError(deps.friendly(msg))
    raise DownloadError(f"yt-dlp failed (exit status: {returncode})")


def final_path(info: dict) -> Path:
    downloads = info.get("requested_downloads")
    fp = None
    if isinstance(downloads, list) and downloads:
        last = downloads[-1]
        if isinstance(last, dict):
            fp = last.get("filepath") if isinstance(last.get("filepath"), str) else None
    if fp is None and isinstance(info.get("filepath"), str):
        fp = info["filepath"]
    if fp is None:
        raise DownloadError("yt-dlp didn't report where the file went")
    return Path(fp)


def str_field(info: dict, key: str) -> str:
    return _as_str(info.get(key)).strip()


def youtube_track_fields(info: dict) -> tuple[str, str]:
    """Best-effort (title, artist) for a yt-dlp info dict. Prefers YouTube's own
    "Music in this video" metadata (artist/track), which is far more reliable
    than the raw video title, then falls back to the uploading channel's name."""

    def first(keys: list[str]) -> str:
        return next((s for s in (str_field(info, k) for k in keys) if s), "")

    title = first(["track", "title"])
    artist = first(["artist", "creator", "channel", "uploader"])
    if artist and " - " in title:
        head, rest = title.split(" - ", 1)
        if head.strip().lower() == artist.lower():
            title = rest.strip()
    return title, artist


def rename_with_artist(path: Path, title: str, artist: str) -> tuple[Path, str]:
    """Renames a downloaded file to the "Artist - Title" convention that
    `name_fields` parses artist/title from, moving its cover art along with it.
    No-ops when no artist is known, the title already leads with it, or the
    file is already gone. Returns (final_path, display_name)."""
    if not artist or title.lower().startswith(artist.lower()) or not path.is_file():
        return path, title
    display = f"{artist} - {title}"
    dest = path.with_name(sanitize_filename(display) + path.suffix)
    if not _same_file(dest, path):
        _remove_quietly(dest)
        try:
            path.rename(dest)
        except OSError:
            return path, title
        old_cover = cover_path_for(path)
        if old_cover.is_file():
            new_cover = cover_path_for(dest)
            _remove_quietly(new_cover)
            try:
                old_cover.rename(new_cover)
            except OSError:
                pass
    return dest, display


def download_youtube(url: str, job: Optional[Job] = None) -> tuple[Path, str]:
    """Downloads a single video, ignoring any ?list= playlist it's part of.
    Returns (path, display title)."""
    info = run_ytdlp(url, job)
    path = final_path(info)
    title, artist = youtube_track_fields(info)
    if not title:
        title = file_stem(path)
    dest, display = rename_with_artist(path, title, artist)
    remember_real_names(dest, title, artist)
    thumb = info.get("thumbnail")
    if isinstance(thumb, str):
        download_image(thumb, cover_path_for(dest))
    vid = info.get("id")
    if isinstance(vid, str):
        record_source(f"yt:{vid}", file_name(dest))
    return dest, display


def download_spotify(url: str, job: Optional[Job] = None) -> tuple[Path, str, bool]:
    """Downloads a single Spotify track link by finding its best match on YouTube.
    Returns (path, display_name, duplicate) - duplicate is True when the track
    was already in the library and nothing was downloaded."""
    key = source_key(url)
    existing = existing_for_source(key) if key else None
    if existing is not None:
        return existing, file_stem(existing), True

    title, artist, cover = spotify.fetch_track_metadata(url)
    if not title:
        raise DownloadError("Could not read that Spotify link - check it's a track URL.")
    display = f"{artist} - {title}" if artist else title
    expected = paths().library / f"{sanitize_filename(display)}.mp3"
    if expected.is_file():
        if key:
            record_source(key, file_name(expected))
        return expected, file_stem(expected), True

    query = f"{artist} {title} audio" if artist else f"{title} audio"
    try:
        info = run_ytdlp(f"ytsearch1:{query}", job)
    except DownloadError as e:
        if "yt-dlp failed" in str(e):
            raise DownloadError(f'No YouTube match found for "{query}"') from e
        raise
    downloaded = final_path(info)
    dest = paths().library / f"{sanitize_filename(display)}.mp3"
    if not _same_file(dest, downloaded):
        _remove_quietly(dest)
        downloaded.rename(dest)
    remember_real_names(dest, title, artist)
    if cover:
        download_image(cover, cover_path_for(dest))
    if key:
        record_source(key, file_name(dest))
    return dest, display, False


def list_youtube_playlist(url: str) -> tuple[Optional[str], list[tuple[str, str]]]:
    """(playlist title, [(video id, title)]) of a YouTube playlist, from a quick
    listing that downloads nothing - so progress has a real total."""
    try:
        out = _run_output([
            "yt-dlp", "--flat-playlist", "--ignore-errors", "--no-warnings",
            "--print", "%(id)s\t%(playlist_title)s\t%(title)s", url,
        ])
    except OSError:
        return None, []
    title: Optional[str] = None
    entries: list[tuple[str, str]] = []
    for line in out.stdout.splitlines():
        f = line.split("\t", 2) + ["", "", ""]
        vid, pl, t = f[0], f[1], f[2]
        if not vid or vid == "NA":
            continue
        if title is None and pl and pl != "NA":
            title = pl
        entries.append((vid, t))
    return title, entries


def search_youtube(query: str) -> list[dict]:
    """Searches YouTube for songs matching `query` (nothing is downloaded).
    Each result: id, url, title, artist (the channel), duration, thumbnail, in_library."""
    try:
        out = _run_output([
            "yt-dlp", "--flat-playlist", "--no-warnings", "--ignore-errors", "--print",
            "%(id)s\t%(duration)s\t%(channel,uploader)s\t%(title)s",
            f"ytsearch12:{query}",
        ])
    except OSError as e:
        raise DownloadError(f"Couldn't run yt-dlp ({e}) - is it installed and on PATH?") from e
    results: list[dict] = []
    for line in out.stdout.splitlines():
        f = line.split("\t", 3) + ["", "", "", ""]
        vid, duration_s, channel, title = f[0], f[1], f[2], f[3]
        try:
            duration = float(duration_s)
        except ValueError:
            duration = 0.0
        # skip live streams / unknown lengths and hour-long mixes
        if len(vid) != 11 or duration <= 0.0 or duration > 1500.0 or not title:
            continue
        artist = channel
        while artist.endswith(" - Topic"):
            artist = artist[: -len(" - Topic")]
        results.append({
            "id": vid,
            "url": f"https://www.youtube.com/watch?v={vid}",
            "title": title,
            "artist": "" if artist == "NA" else artist,
            "duration": duration,
            "thumbnail": f"https://i.ytimg.com/vi/{vid}/mqdefault.jpg",
            "in_library": existing_for_source(f"yt:{vid}") is not None,
        })
    if not results and out.returncode != 0:
        raise DownloadError("Couldn't search right now. Check your connection and try again.")
    return results


def search_catalog(query: str) -> list[dict]:
    """Searches Apple's public music catalog (no key needed) for songs matching
    `query`. Used for clean artist/title/album art; the audio itself is matched
    on YouTube when the song is downloaded."""
    url = (
        "https://itunes.apple.com/search?media=music&entity=song&limit=50&term="
        + util.quote_all(query)
    )
    try:
        text = util.http_get_text(url, 15, 2)
    except Exception as e:
        raise DownloadError("Couldn't reach the music catalog. Check your connection and try again.") from e
    try:
        v = json.loads(text)
    except ValueError as e:
        raise DownloadError(str(e)) from e

    results: list[dict] = []
    # the same song often comes back once per album, single and compilation: keep the first
    seen: set[str] = set()
    raw = v.get("results") if isinstance(v, dict) else None
    for r in raw if isinstance(raw, list) else []:
        if not isinstance(r, dict):
            continue
        track_id, title = r.get("trackId"), r.get("trackName")
        if not isinstance(track_id, int) or isinstance(track_id, bool) or not isinstance(title, str):
            continue
        artist = _as_str(r.get("artistName"))
        same = "".join(c for c in f"{artist}\x01{title}".lower() if c.isalnum() or c == "\x01")
        if same in seen:
            continue
        seen.add(same)
        if len(results) >= 30:
            break
        duration = _as_float(r.get("trackTimeMillis")) / 1000.0
        art = _as_str(r.get("artworkUrl100"))
        key = f"apple:{track_id}"
        album = r.get("collectionName")
        results.append({
            "kind": "catalog",
            "id": key,
            "title": title,
            "artist": artist,
            "album": album,
            "duration": duration,
            "thumbnail": art.replace("100x100bb", "200x200bb"),
            "in_library": catalog_existing(key, artist, title) is not None,
            "catalog": {
                "id": key, "title": title, "artist": artist, "album": album, "duration": duration,
                "cover": art.replace("100x100bb", "600x600bb"),
            },
        })
    return results


def catalog_in_library(key: str, artist: str, title: str) -> bool:
    return catalog_existing(key, artist, title) is not None


def catalog_display(title: str, artist: str) -> str:
    return f"{artist} - {title}" if artist else title


def catalog_existing(key: str, artist: str, title: str) -> Optional[Path]:
    existing = existing_for_source(key)
    if existing is not None:
        return existing
    p = paths().library / f"{sanitize_filename(catalog_display(title, artist))}.mp3"
    return p if p.is_file() else None


def download_catalog(item: dict, job: Job) -> tuple[Path, str, bool]:
    """Downloads a catalog song by finding its best audio match on YouTube:
    among the top results, the one whose length is closest to the song's.
    Returns (path, display_name, duplicate)."""
    key = _as_str(item.get("id"))
    title = _as_str(item.get("title")).strip()
    artist = _as_str(item.get("artist")).strip()
    if not key or not title:
        raise DownloadError("That song is missing its details.")
    existing = catalog_existing(key, artist, title)
    if existing is not None:
        record_source(key, file_name(existing))
        return existing, file_stem(existing), True

    query = f"{artist} {title} audio" if artist else f"{title} audio"
    target = _as_float(item.get("duration"))
    try:
        matches = search_youtube(query)
    except Exception:
        matches = []

    def distance(m: dict) -> float:
        return abs(_as_float(m.get("duration")) - target)

    close = [m for m in matches if target <= 0.0 or distance(m) <= 12.0]
    best = min(close, key=distance) if close else (matches[0] if matches else None)
    target_url = best["url"] if best and isinstance(best.get("url"), str) else f"ytsearch1:{query}"

    info = run_ytdlp(target_url, job)
    downloaded = final_path(info)
    display = catalog_display(title, artist)
    dest = paths().library / f"{sanitize_filename(display)}.mp3"
    if not _same_file(dest, downloaded):
        _remove_quietly(dest)
        downloaded.rename(dest)
    remember_real_names(dest, title, artist)
    album = _as_str(item.get("album"))
    if album.strip():
        library.remember_album(dest, album)
    cover = _as_str(item.get("cover"))
    if cover:
        download_image(cover, cover_path_for(dest))
    record_source(key, file_name(dest))
    vid = info.get("id")
    if isinstance(vid, str):
        record_source(f"yt:{vid}", file_name(dest))
    return dest, display, False


def start_catalog(item: dict) -> str:
    """Starts a download job for a catalog song; returns the job id."""
    job = new_download_job("track")
    job_id = register(job)

    def worker() -> None:
        paths().library.mkdir(parents=True, exist_ok=True)
        paths().covers.mkdir(parents=True, exist_ok=True)
        new_ids: list[str] = []
        try:
            path, title, duplicate = download_catalog(item, job)
            job.update(duplicate=duplicate, title=title, id=file_name(path))
            if not duplicate:
                job.update(percent="Fetching lyrics...")
                lyrics.fetch_with_song(path)
                new_ids.append(file_name(path))
            job.update(status="done")
        except Exception as e:
            store.error(f"Catalog download failed for {json.dumps(item.get('title'))}: {e}")
            job.update(status="error", error=str(e))
        library.scan_loudness_async()
        library.backfill_albums_async()
        lyrics.prefetch_async(new_ids)

    threading.Thread(target=worker, daemon=True).start()
    return job_id


# ---------------- import jobs ----------------


def finish_track(job: Job, playlist_id: Optional[str], path: Path) -> None:
    if playlist_id is not None:
        add_track_to_playlist_record(playlist_id, file_name(path))
    job.update(percent="Fetching lyrics...")
    lyrics.fetch_with_song(path)
    job.update(fraction=0.0, percent="")


def run_playlist_import(url: str, job: Job, as_playlist: bool) -> None:
    """Downloads every track of a playlist link. With `as_playlist`, the tracks are
    also collected into a new local playlist; otherwise they only join the library."""
    job.update(as_playlist=as_playlist)

    if is_spotify_playlist_url(url):
        title, track_urls = spotify.fetch_playlist_tracks(url)
        if not track_urls:
            raise DownloadError("Could not read tracks from that Spotify playlist link.")
        name = title or "Imported Playlist"
        playlist = create_playlist_record(name) if as_playlist else None
        pid = playlist.get("id") if playlist else None
        pid = pid if isinstance(pid, str) else None
        job.update(
            playlist_id=pid,
            playlist_name=name if as_playlist else (title or "Spotify playlist"),
            total=len(track_urls),
        )
        for track_url in track_urls:
            job.update(current_title=None)
            try:
                path, track_title, duplicate = download_spotify(track_url, job)
            except Exception as e:
                store.warn(f"Download failed for {track_url}: {e}")
                job.push("failed", {"title": track_url, "error": str(e)})
                continue
            job.update(current_title=track_title)
            if duplicate:
                if pid is not None:
                    add_track_to_playlist_record(pid, file_name(path))
                job.update(fraction=0.0, percent="")
            else:
                finish_track(job, pid, path)
            job.push("skipped" if duplicate else "added", {"id": file_name(path), "title": track_title})
        return

    playlist = create_playlist_record("Importing playlist...") if as_playlist else None
    pid = playlist.get("id") if playlist else None
    pid = pid if isinstance(pid, str) else None
    job.update(
        playlist_id=pid,
        playlist_name="Importing playlist..." if as_playlist else "YouTube playlist",
    )
    playlist_title, entries = list_youtube_playlist(url)
    job.update(total=len(entries))
    for vid, entry_title in entries:
        existing = existing_for_source(f"yt:{vid}")
        if existing is not None:
            # Already downloaded: still belongs in the playlist, just don't re-fetch it.
            if pid is not None:
                add_track_to_playlist_record(pid, file_name(existing))
            job.push("skipped", {"id": file_name(existing), "title": file_stem(existing)})
            continue
        job.update(current_title=entry_title, fraction=0.0)
        try:
            path, title = download_youtube(f"https://www.youtube.com/watch?v={vid}", job)
        except Exception as e:
            store.warn(f"Download failed for {vid}: {e}")
            job.push("failed", {"title": entry_title, "error": str(e)})
            continue
        finish_track(job, pid, path)
        job.push("added", {"id": file_name(path), "title": title})

    final_name = playlist_title or "Imported Playlist"
    if pid is not None:
        rename_playlist_record(pid, final_name)
    job.update(playlist_name=final_name)
    empty = job.with_(lambda j: not j["added"] and not j["skipped"])
    if empty:
        raise DownloadError("Could not read any tracks from that YouTube playlist link.")


def run_single(url: str, job: Job) -> Optional[str]:
    existing = None
    if not is_spotify_url(url):
        key = source_key(url)
        existing = existing_for_source(key) if key else None

    if is_spotify_url(url):
        path, title, duplicate = download_spotify(url, job)
    elif existing is not None:
        path, title, duplicate = existing, file_stem(existing), True
    else:
        path, title = download_youtube(url, job)
        duplicate = False

    job.update(duplicate=duplicate, title=title, id=file_name(path))
    if duplicate:
        return None
    job.update(percent="Fetching lyrics...")
    lyrics.fetch_with_song(path)
    return file_name(path)


def start(url: str, mode: str, as_playlist: bool) -> str:
    """Starts a download job on a background thread; returns its id."""
    job = new_download_job("playlist" if mode == "playlist" else "track")
    job_id = register(job)

    def worker() -> None:
        paths().library.mkdir(parents=True, exist_ok=True)
        paths().covers.mkdir(parents=True, exist_ok=True)
        new_ids: list[str] = []  # songs this job added, for a catch-up lyrics pass
        try:
            if mode == "playlist":
                run_playlist_import(url, job, as_playlist)
            else:
                added = run_single(url, job)
                if added is not None:
                    new_ids.append(added)
            job.update(status="done")
        except Exception as e:
            store.error(f"Download job failed for {url}: {e}")
            job.update(status="error", error=str(e))
        if mode == "playlist":
            new_ids = job.with_(
                lambda j: [i["id"] for i in j["added"] if isinstance(i.get("id"), str)]
            )
        library.scan_loudness_async()  # measure whatever was just added
        library.backfill_albums_async()
        lyrics.prefetch_async(new_ids)  # retries any lookups that failed while offline

    threading.Thread(target=worker, daemon=True).start()
    return job_id