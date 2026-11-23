'use strict';
/* Vinyl: a Spotify-style front end for the local library.
   Likes, play history and playlists come from the Rust server; recommendations are
   computed here from those plus play counts. */

const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const enc = encodeURIComponent;
const lsGet = (k, d) => { try { const v = localStorage.getItem(k); return v === null ? d : JSON.parse(v); } catch { return d; } };
const lsSet = (k, v) => { try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* private mode */ } };

async function api(path, opts = {}) {
  const o = { method: opts.method || 'GET', headers: {} };
  if (opts.body !== undefined) { o.body = JSON.stringify(opts.body); o.headers['Content-Type'] = 'application/json'; }
  const r = await fetch(path, o);
  if (!r.ok) { let m = r.statusText; try { m = (await r.json()).error || m; } catch { /* no body */ } throw new Error(m); }
  return r.status === 204 ? null : r.json();
}

const ICON = {
  pin: '<svg viewBox="0 0 24 24"><path d="M14.5 3.5l6 6-2 1-2.2 2.2.5 4.3-1.3 1.3-3.3-3.3-5.2 5.2-1-1 5.2-5.2-3.3-3.3 1.3-1.3 4.3.5L13.5 5.5z"/></svg>',
  play: '<svg viewBox="0 0 24 24"><path d="M7 4.5v15l13-7.5z"/></svg>',
  pause: '<svg viewBox="0 0 24 24"><path d="M6 4.5h4v15H6zM14 4.5h4v15h-4z"/></svg>',
  heart: '<svg viewBox="0 0 24 24"><path d="M12 20.5s-8.5-5-8.5-11.2A4.7 4.7 0 0 1 12 6.6a4.7 4.7 0 0 1 8.5 2.7c0 6.2-8.5 11.2-8.5 11.2z"/></svg>',
  heartFill: '<svg viewBox="0 0 24 24"><path d="M12 20.5s-8.5-5-8.5-11.2A4.7 4.7 0 0 1 12 6.6a4.7 4.7 0 0 1 8.5 2.7c0 6.2-8.5 11.2-8.5 11.2z"/></svg>',
  more: '<svg viewBox="0 0 24 24"><circle cx="5" cy="12" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="19" cy="12" r="2"/></svg>',
  plus: '<svg viewBox="0 0 24 24"><path d="M11 4h2v7h7v2h-7v7h-2v-7H4v-2h7z"/></svg>',
  check: '<svg viewBox="0 0 24 24"><path d="m4 12.5 5 5L20 6.5" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  shuffle: '<svg viewBox="0 0 24 24"><path d="M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  repeat: '<svg viewBox="0 0 24 24"><path d="M17 2l4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4M21 13v2a3 3 0 0 1-3 3H3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  repeat1: '<svg viewBox="0 0 24 24"><path d="M17 2l4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4M21 13v2a3 3 0 0 1-3 3H3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="M11 10l1.5-1v6" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  vol: '<svg viewBox="0 0 24 24"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a8 8 0 0 1 0 11" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>',
  mute: '<svg viewBox="0 0 24 24"><path d="M4 9.5h3.5L12 5.5v13l-4.5-4H4z"/><path d="m16 9.5 5 5m0-5-5 5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>',
  queue: '<svg viewBox="0 0 24 24"><path d="M3 6h13M3 11h13M3 16h8M17 14v6l5-3z" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  user: '<svg viewBox="0 0 24 24"><circle cx="12" cy="8" r="4"/><path d="M4 21c0-4.4 3.6-7 8-7s8 2.6 8 7z"/></svg>',
  radio: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="2.4"/><path d="M7.5 7.5a6.4 6.4 0 0 0 0 9M16.5 7.5a6.4 6.4 0 0 1 0 9M4.6 4.6a10.5 10.5 0 0 0 0 14.8M19.4 4.6a10.5 10.5 0 0 1 0 14.8" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>',
  trash: '<svg viewBox="0 0 24 24"><path d="M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
  edit: '<svg viewBox="0 0 24 24"><path d="m4 20 1-4L16.5 4.5l3 3L8 19zM14 7l3 3" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/></svg>',
  x: '<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/></svg>',
  image: '<svg viewBox="0 0 24 24"><path d="M4 5h16v14H4zM4 16l5-5 4 4 3-3 4 4" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"/><circle cx="9" cy="9" r="1.6"/></svg>',
  list: '<svg viewBox="0 0 24 24"><path d="M4 6h16M4 11h10M4 16h13" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"/></svg>',
};

/* ------------------------------------------------------------------ state */

const S = {
  tracks: [], by: new Map(), likes: new Map(), history: [], playlists: [],
  ver: 0, cache: {}, lists: {}, sort: lsGet('vinyl.sort', 'added'), libFilter: 'all',
  recs: lsGet('vinyl.recs', null), q: '', web: new Map(), pv: new Map(), webItems: {}, picked: new Set(), jobs: [], np: lsGet('vinyl.np', false), lyrics: lsGet('vinyl.lyrics', true),
};
const P = { order: [], pos: -1, ctx: [], userQ: [], shuffle: false, repeat: 'off', cur: null, recorded: false, vol: 70, label: '' };
const audio = $('#audio');

const trackOf = id => S.by.get(id) || S.pv.get(id);
/* Songs you don't own are playable as 30 s previews: they live in S.pv under a "pv:" id. */
function registerPv(it) {
  const id = 'pv:' + it.id;
  S.webItems[it.id] = it;
  S.pv.set(id, { id, title: it.title, artist: it.artist, duration: it.preview ? 30 : it.duration, cover: it.thumbnail, preview: it.preview, plays: 0, added: 0, isPv: true, item: it });
  return id;
}
const stash = items => { items.forEach(i => { S.webItems[i.id] = i; }); return items; };
const isLiked = id => S.likes.has(id);
const fmt = s => { s = Math.max(0, Math.round(s || 0)); const h = Math.floor(s / 3600), m = Math.floor(s % 3600 / 60), x = s % 60; return h ? `${h}:${String(m).padStart(2, '0')}:${String(x).padStart(2, '0')}` : `${m}:${String(x).padStart(2, '0')}`; };
const fmtLong = s => { const m = Math.round(s / 60); return m >= 60 ? `${Math.floor(m / 60)} hr ${m % 60} min` : `${m} min`; };
const plural = (n, w) => `${n.toLocaleString()} ${w}${n === 1 ? '' : 's'}`;
const hash32 = s => { let h = 2166136261; for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619); return h >>> 0; };
const rng = seed => () => { seed |= 0; seed = seed + 0x6D2B79F5 | 0; let t = Math.imul(seed ^ seed >>> 15, 1 | seed); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; };
const shuffled = (arr, r = Math.random) => { const a = arr.slice(); for (let i = a.length - 1; i > 0; i--) { const j = Math.floor(r() * (i + 1)); [a[i], a[j]] = [a[j], a[i]]; } return a; };

/* One toast per message: repeating it (clicking "Check for updates" over and over) restarts the same toast instead of stacking another. */
function toast(msg) {
  const box = $('#toasts');
  let el = [...box.children].find(t => t.textContent === msg);
  if (el) { clearTimeout(el._t); el.style.animation = 'none'; void el.offsetWidth; el.style.animation = ''; }
  else { el = document.createElement('div'); el.className = 'toast'; el.textContent = msg; box.append(el); }
  el._t = setTimeout(() => el.remove(), 2800);
}

/* ------------------------------------------------------------------ data */

async function fetchData() {
  const [tracks, likes, history, playlists] = await Promise.all([
    api('/api/library'), api('/api/likes').catch(() => []), api('/api/history').catch(() => []), api('/api/playlists').catch(() => []),
  ]);
  return { tracks, likes, history, playlists };
}
function applyData(d) {
  S.tracks = d.tracks;
  S.by = new Map(d.tracks.map(t => [t.id, t]));
  S.likes = new Map(d.likes.filter(l => S.by.has(l.id)).map(l => [l.id, l.at]));
  S.history = d.history.filter(h => S.by.has(h.id));
  S.playlists = pinSort(d.playlists);
  S.ver++;
}
async function loadAll() {
  const d = await fetchData();
  applyData(d);
  lsSet('vinyl.data', d); // the next start paints from this while the server is still answering
}
/* What you can see on screen: if this hasn't changed since the cached copy, there is nothing to redraw. */
const dataSig = d => JSON.stringify([d.tracks.map(t => [t.id, t.title, t.artist, t.album, t.cover]), d.likes.map(l => l.id), d.playlists.map(p => [p.id, p.name, p.track_ids, p.cover, p.pinned])]);

/* pinned playlists come first (each group keeps its own order) */
const pinSort = list => [...list.filter(p => p.pinned), ...list.filter(p => !p.pinned)];
async function refreshPlaylists() { S.playlists = pinSort(await api('/api/playlists')); S.ver++; renderLibrary(); }

async function toggleLike(id, force) {
  const pt = trackOf(id); if (pt?.isPv) { addPv(pt); return; }
  const want = force ?? !isLiked(id);
  if (want) S.likes.set(id, Date.now() / 1000); else S.likes.delete(id);
  S.ver++;
  syncHearts();
  renderLibrary();
  try { await api(`/api/likes/${enc(id)}`, { method: want ? 'PUT' : 'DELETE' }); }
  catch (e) { if (want) S.likes.delete(id); else S.likes.set(id, Date.now() / 1000); syncHearts(); toast("Couldn't update Liked Songs"); return; }
  toast(want ? 'Added to Liked Songs' : 'Removed from Liked Songs');
  if (route.name === 'liked') renderRoute(true);
}

/* --------------------------------------------------------------- artwork */

function hueOf(t) { return hash32((t.artist || '') + (t.title || '')) % 360; }
function artHTML(t, cls = '') {
  if (!t) return `<div class="art ${cls}"></div>`;
  if (t.cover) return `<div class="art ${cls}"><img class="art-img" src="${esc(t.cover)}" alt="" loading="lazy" decoding="async" onload="this.classList.add('in')" onerror="this.remove()"></div>`;
  const h = hueOf(t);
  return `<div class="art ${cls}" style="background:linear-gradient(135deg,hsl(${h} 45% 34%),hsl(${(h + 50) % 360} 50% 20%))"><span class="ph">${esc((t.title || '?').trim().charAt(0).toUpperCase())}</span></div>`;
}
function mosaicHTML(ids, cls = '') {
  const ts = ids.map(trackOf).filter(Boolean);
  if (!ts.length) return `<div class="art ${cls}"></div>`;
  if (ts.length < 4) return artHTML(ts[0], cls);
  return `<div class="art ${cls}"><div class="mosaic">${ts.slice(0, 4).map(t => t.cover ? `<div><img class="art-img" src="${esc(t.cover)}" alt="" loading="lazy" decoding="async" onload="this.classList.add('in')" onerror="this.remove()"></div>` : `<div style="background:hsl(${hueOf(t)} 40% 28%)"></div>`).join('')}</div></div>`;
}
new MutationObserver(muts => {
  for (const m of muts) for (const n of m.addedNodes) {
    if (n.nodeType !== 1) continue;
    for (const img of (n.matches?.('img.art-img') ? [n] : n.querySelectorAll?.('img.art-img') || [])) {
      if (img.complete && img.naturalWidth) img.classList.add('in', 'nofade');
    }
  }
}).observe(document.body, { childList: true, subtree: true });
const likedArt = cls => `<div class="art liked ${cls}">${ICON.heartFill}</div>`;

const colorCache = new Map();
function coverColor(url, k = 0.62) {
  if (!url) return Promise.resolve(null);
  const ck = url + '|' + k;
  if (colorCache.has(ck)) return Promise.resolve(colorCache.get(ck));
  return new Promise(res => {
    const img = new Image();
    img.onload = () => {
      try {
        const c = document.createElement('canvas'); c.width = c.height = 12;
        const x = c.getContext('2d', { willReadFrequently: true }); x.drawImage(img, 0, 0, 12, 12);
        const d = x.getImageData(0, 0, 12, 12).data;
        let r = 0, g = 0, b = 0, w = 0;
        for (let i = 0; i < d.length; i += 4) {
          const mx = Math.max(d[i], d[i + 1], d[i + 2]), mn = Math.min(d[i], d[i + 1], d[i + 2]);
          const k = 0.15 + (mx - mn) / 255; // favour saturated pixels
          r += d[i] * k; g += d[i + 1] * k; b += d[i + 2] * k; w += k;
        }
        const f = k / Math.max(0.01, w); // darken so text stays readable
        const col = `rgb(${Math.round(r * f)},${Math.round(g * f)},${Math.round(b * f)})`;
        colorCache.set(ck, col); res(col);
      } catch { res(null); }
    };
    img.onerror = () => res(null);
    img.src = url;
  });
}
async function tint(ids) {
  for (const id of ids) {
    const t = trackOf(id);
    const col = t && await coverColor(t.cover);
    if (col) { $('#view').style.setProperty('--hue', col); return; }
  }
  $('#view').style.setProperty('--hue', '#3a2a22');
}

/* ---------------------------------------------------- recommendation engine */

const primaryArtist = a => (a || 'Unknown').split(/\s*(?:,|&|\bfeat\.?|\bft\.?|\bwith\b)\s*/i)[0].trim() || 'Unknown';
const akey = t => primaryArtist(t.artist).toLowerCase();

/* Builds (and caches per data version) the taste model:
   - affinity: how much you like each artist (plays, likes, recent listening)
   - sim: how related two artists are (shared playlists, played back to back,
     liked together, added in the same import) */
function taste() {
  if (S.cache.taste?.ver === S.ver) return S.cache.taste;
  const now = Date.now() / 1000, aff = new Map(), sim = new Map(), names = new Map(), byArtist = new Map();
  const bump = (m, k, v) => m.set(k, (m.get(k) || 0) + v);
  for (const t of S.tracks) {
    const k = akey(t);
    names.set(k, primaryArtist(t.artist));
    (byArtist.get(k) || byArtist.set(k, []).get(k)).push(t);
    bump(aff, k, 1.2 * Math.log1p(t.plays || 0) + (isLiked(t.id) ? 4 : 0));
  }
  for (const h of S.history) {
    const t = trackOf(h.id); if (!t) continue;
    bump(aff, akey(t), 2 * Math.exp(-(now - h.at) / 86400 / 14));
  }
  const link = (a, b, w) => {
    if (a === b) return;
    for (const [x, y] of [[a, b], [b, a]]) { const m = sim.get(x) || sim.set(x, new Map()).get(x); m.set(y, (m.get(y) || 0) + w); }
  };
  for (const pl of S.playlists) {
    const ks = [...new Set((pl.track_ids || []).map(trackOf).filter(Boolean).map(akey))];
    const w = 1 / Math.sqrt(Math.max(1, ks.length));
    for (let i = 0; i < ks.length; i++) for (let j = i + 1; j < ks.length; j++) link(ks[i], ks[j], w);
  }
  for (let i = 1; i < S.history.length; i++) {
    const a = trackOf(S.history[i - 1].id), b = trackOf(S.history[i].id);
    if (a && b && S.history[i].at - S.history[i - 1].at < 1800) link(akey(a), akey(b), 1);
  }
  const likedKs = [...new Set([...S.likes.keys()].map(trackOf).filter(Boolean).map(akey))];
  for (let i = 0; i < likedKs.length; i++) for (let j = i + 1; j < likedKs.length; j++) link(likedKs[i], likedKs[j], 0.3);
  const byAdded = S.tracks.slice().sort((a, b) => a.added - b.added); // same import batch -> likely same taste
  for (let i = 0; i < byAdded.length; i++) for (let j = i + 1; j < Math.min(byAdded.length, i + 4); j++) link(akey(byAdded[i]), akey(byAdded[j]), 0.35);
  const top = [...aff.entries()].filter(([, v]) => v > 0).sort((a, b) => b[1] - a[1]).map(([k]) => k);
  const maxAff = Math.max(1, ...aff.values());
  return (S.cache.taste = { ver: S.ver, aff, sim, names, byArtist, top, maxAff });
}

const dayRng = salt => rng(hash32(new Date().toDateString() + salt));

function relatedArtists(k, n = 8) {
  const m = taste().sim.get(k); if (!m) return [];
  return [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, n).map(([x]) => x);
}

/* Scores every track for a set of seed artists (weights). Unheard songs get a bump,
   very recent plays a penalty, and a day-seeded jitter keeps mixes fresh but stable. */
function scoreTracks(seeds, { r = dayRng('x'), exclude = new Set(), novelty = 0.35 } = {}) {
  const T = taste(), now = Date.now() / 1000, lastPlay = new Map();
  for (const h of S.history) lastPlay.set(h.id, h.at);
  const out = [];
  for (const t of S.tracks) {
    if (exclude.has(t.id)) continue;
    const k = akey(t); let s = 0;
    for (const [sk, w] of seeds) {
      if (sk === k) s += w;
      else s += w * 0.7 * Math.min(1, (T.sim.get(sk)?.get(k) || 0) / 2);
    }
    s += 0.25 * (T.aff.get(k) || 0) / T.maxAff;
    if (isLiked(t.id)) s += 0.25;
    if (!t.plays) s += novelty;
    const lp = lastPlay.get(t.id);
    if (lp && now - lp < 3 * 3600) s -= 0.5;
    out.push({ t, s: s + r() * 0.3 });
  }
  return out.sort((a, b) => b.s - a.s);
}

/* Interleaves so the same artist doesn't fill a whole run of the list. */
function spread(scored, n, maxRun = 2) {
  const out = [], left = scored.slice();
  while (out.length < n && left.length) {
    let i = 0;
    const last = out.slice(-maxRun).map(t => akey(t));
    if (last.length === maxRun && last.every(k => k === last[0])) {
      const j = left.findIndex(x => akey(x.t) !== last[0]); if (j >= 0) i = j;
    }
    out.push(left.splice(i, 1)[0].t);
  }
  return out;
}

function similarTo(track, n = 20) {
  const T = taste(), k = akey(track), seeds = new Map([[k, 1]]);
  for (const r of relatedArtists(k, 6)) seeds.set(r, 0.8);
  const scored = scoreTracks(seeds, { r: Math.random, exclude: new Set([track.id]), novelty: 0.1 });
  for (const x of scored) { // closeness in loudness/length is a faint "vibe" signal
    if (track.gain_db != null && x.t.gain_db != null) x.s += 0.15 * Math.max(0, 1 - Math.abs(track.gain_db - x.t.gain_db) / 8);
    x.s += 0.2 * Math.exp(-Math.abs(track.added - x.t.added) / 900);
  }
  scored.sort((a, b) => b.s - a.s);
  return spread(scored, n, 3);
}

function mixes() {
  if (S.cache.mixes?.ver === S.ver) return S.cache.mixes.list;
  const T = taste(), list = [], used = new Set();
  const tops = T.top.slice(0, 12);
  const count = Math.min(3, Math.max(0, Math.ceil(S.tracks.length / 8)), tops.length);
  for (let i = 0; i < count; i++) {
    const seed = tops.find(k => !used.has(k)); if (!seed) break;
    const group = [seed, ...relatedArtists(seed, 3).filter(k => !used.has(k)).slice(0, 2)];
    group.forEach(k => used.add(k));
    const seeds = new Map(group.map((k, j) => [k, j ? 0.8 : 1]));
    const ids = spread(scoreTracks(seeds, { r: dayRng('mix' + i) }), 30).map(t => t.id);
    if (ids.length >= 3) list.push({ n: i, name: `Daily Mix ${i + 1}`, ids, artists: group.map(k => T.names.get(k)) });
  }
  S.cache.mixes = { ver: S.ver, list };
  return list;
}

function shelves() {
  const T = taste(), now = Date.now() / 1000, out = {};
  const seenRecent = [];
  for (let i = S.history.length - 1; i >= 0 && seenRecent.length < 16; i--) if (!seenRecent.includes(S.history[i].id)) seenRecent.push(S.history[i].id);
  out.recent = seenRecent;
  out.mixes = mixes();
  out.topArtists = T.top.slice(0, 12);
  const k0 = T.top[0];
  if (k0) {
    const seeds = new Map([[k0, 0.6], ...relatedArtists(k0, 5).map(k => [k, 1])]);
    out.because = { artist: T.names.get(k0), ids: spread(scoreTracks(seeds, { exclude: new Set(S.tracks.filter(t => akey(t) === k0).map(t => t.id)) }), 12).map(t => t.id) };
    const k1 = T.top.find(k => k !== k0 && k !== T.top[0]) || null;
    if (k1) out.because2 = { artist: T.names.get(k1), ids: spread(scoreTracks(new Map([[k1, 1], ...relatedArtists(k1, 4).map(k => [k, 0.8])]), { exclude: new Set(S.tracks.filter(t => akey(t) === k1).map(t => t.id)) }), 12).map(t => t.id) };
  }
  const seeds = new Map(T.top.slice(0, 8).map((k, i) => [k, 1 - i * 0.08]));
  out.discover = spread(scoreTracks(seeds, { exclude: new Set(S.tracks.filter(t => t.plays || isLiked(t.id)).map(t => t.id)), novelty: 0 }), 12).map(t => t.id);
  out.onRepeat = S.tracks.map(t => {
    let s = 0; for (const h of S.history) if (h.id === t.id) s += Math.exp(-(now - h.at) / 86400 / 10);
    return [t.id, s || (t.plays || 0) * 0.2];
  }).filter(([, s]) => s > 0.15).sort((a, b) => b[1] - a[1]).slice(0, 12).map(([id]) => id);
  const lastAt = new Map(S.history.map(h => [h.id, h.at]));
  out.rediscover = [...S.likes.keys()].filter(id => now - (lastAt.get(id) || 0) > 14 * 86400).sort(() => dayRng('re')() - 0.5).slice(0, 12);
  out.added = S.tracks.slice().sort((a, b) => b.added - a.added).slice(0, 12).map(t => t.id);
  return out;
}

/* --------------------------------------------------------------- playback */

function applyVolume() {
  let v = (P.vol / 100) ** 2;
  if (P.cur && P.cur.gain_db != null) v *= Math.min(1, 10 ** (P.cur.gain_db / 20));
  audio.volume = Math.max(0, Math.min(1, v));
  $('#vol').value = P.vol; $('#vol').style.setProperty('--p', P.vol + '%');
  $('#b-mute').innerHTML = P.vol === 0 ? ICON.mute : ICON.vol;
}

function setContext(ids, start = 0, { shuffle = P.shuffle, label = '' } = {}) {
  ids = ids.filter(id => trackOf(id));
  if (!ids.length) return;
  P.ctx = ids; P.label = label; P.userQ = P.userQ.slice();
  if (shuffle !== P.shuffle) { P.shuffle = shuffle; }
  const first = ids[start] ?? ids[0];
  P.order = P.shuffle ? [first, ...shuffled(ids.filter(x => x !== first))] : ids.slice();
  P.pos = P.shuffle ? 0 : Math.max(0, start);
  syncControls();
  loadTrack(P.order[P.pos], true);
}

function loadTrack(id, autoplay, resumeAt = 0) {
  const t = trackOf(id); if (!t) return;
  P.cur = t; P.recorded = !!t.isPv;
  AsciiVinyl.setCover(t.cover);
  lsSet('vinyl.cover', t.cover || null);
  coverColor(t.cover, 1.05).then(c => { $('#stage').style.setProperty('--tint', c || '#2c2c2f'); if (c) lsSet('vinyl.tint', c); });
  if (t.isPv) { // not in the library yet: download it, then play the real file
    audio.pause(); audio.removeAttribute('src');
    renderBarTrack(); renderNP(); markNow();
    toast(`Downloading ${t.title}…`);
    downloadPv(t).then(real => {
      if (P.cur?.id !== t.id) return; // the user moved on
      if (real) loadTrack(real, autoplay);
      else { toast(`Couldn't download ${t.title}`); next(false); }
    });
    return;
  }
  audio.src = `/audio/${enc(t.id)}`;
  applyVolume();
  if (resumeAt) audio.addEventListener('loadedmetadata', () => { audio.currentTime = resumeAt; }, { once: true });
  if (autoplay) audio.play().catch(() => { });
  renderBarTrack(); renderNP(); markNow(); mediaSession(); saveStateSoon();
}

function togglePlay() {
  if (!P.cur) { const all = S.tracks.map(t => t.id); if (all.length) setContext(shuffled(all), 0, { shuffle: false, label: 'Your library' }); return; }
  if (audio.paused) audio.play().catch(() => { }); else audio.pause();
}

function next(auto = false) {
  if (P.repeat === 'one' && auto) { audio.currentTime = 0; audio.play().catch(() => { }); return; }
  if (P.userQ.length) { loadTrack(P.userQ.shift(), true); return; }
  if (P.pos + 1 < P.order.length) { P.pos++; loadTrack(P.order[P.pos], true); return; }
  if (P.repeat === 'all' && P.order.length) { P.pos = 0; loadTrack(P.order[0], true); return; }
  audio.pause();
}

function prev() {
  if (audio.currentTime > 3 || P.pos <= 0) { audio.currentTime = 0; return; }
  P.pos--; loadTrack(P.order[P.pos], true);
}

function toggleShuffle() {
  P.shuffle = !P.shuffle;
  const cur = P.cur?.id;
  if (P.shuffle) { P.order = [cur, ...shuffled(P.ctx.filter(x => x !== cur))].filter(Boolean); P.pos = 0; }
  else { P.order = P.ctx.slice(); P.pos = Math.max(0, P.order.indexOf(cur)); }
  syncControls(); renderNP(); saveStateSoon();
}

function queueNext(ids) { P.userQ.unshift(...ids); toast(ids.length > 1 ? `${ids.length} songs play next` : 'Plays next'); renderNP(); if (route.name === 'queue') renderRoute(true); saveStateSoon(); }
function queueEnd(ids) { P.userQ.push(...ids); toast(ids.length > 1 ? `${ids.length} songs added to queue` : 'Added to queue'); renderNP(); if (route.name === 'queue') renderRoute(true); saveStateSoon(); }

function startRadio(t) {
  const ids = [t.id, ...similarTo(t, 25).map(x => x.id)];
  setContext(ids, 0, { shuffle: false, label: `${t.title} radio` });
  toast(`Started radio for ${t.title}`);
}

function syncControls() {
  $('#b-shuffle').classList.toggle('on', P.shuffle);
  const rp = $('#b-repeat');
  rp.innerHTML = P.repeat === 'one' ? ICON.repeat1 : ICON.repeat;
  rp.classList.toggle('on', P.repeat !== 'off');
  rp.title = { off: 'Repeat', all: 'Repeat all', one: 'Repeat one' }[P.repeat];
  $('#b-lyrics').classList.toggle('on', S.np && S.lyrics);
  $('#toggle-np').classList.toggle('on', S.np);
}

audio.addEventListener('play', () => { try { navigator.mediaSession.playbackState = 'playing'; } catch { /* unsupported */ } document.body.classList.add('playing'); document.body.classList.remove('paused'); $('#b-play').innerHTML = ICON.pause; $('#b-play').title = 'Pause'; });
audio.addEventListener('pause', () => { try { navigator.mediaSession.playbackState = 'paused'; } catch { /* unsupported */ } document.body.classList.remove('playing'); document.body.classList.add('paused'); $('#b-play').innerHTML = ICON.play; $('#b-play').title = 'Play'; });
audio.addEventListener('ended', () => next(true));
audio.addEventListener('error', () => { if (P.cur) { toast(`Couldn't play ${P.cur.title}`); setTimeout(() => next(true), 600); } });
let seeking = false;
audio.addEventListener('timeupdate', () => {
  const d = audio.duration || P.cur?.duration || 0, c = audio.currentTime;
  $('#t-cur').textContent = fmt(c); $('#t-dur').textContent = fmt(d);
  if (!seeking && d) { const p = c / d * 1000; $('#seek').value = p; $('#seek').style.setProperty('--p', p / 10 + '%'); }
  if (P.cur && !P.recorded && d && c >= Math.min(30, d / 2)) recordPlay(P.cur);
  highlightLyric(c);
  saveStateSoon(4000);
});

function recordPlay(t) {
  P.recorded = true;
  t.plays = (t.plays || 0) + 1;
  S.history.push({ id: t.id, at: Date.now() / 1000 }); S.ver++;
  api(`/api/tracks/${enc(t.id)}/play`, { method: 'POST' }).catch(() => { });
}

$('#b-play').onclick = togglePlay;
$('#b-next').onclick = () => next(false);
$('#b-prev').onclick = prev;
$('#b-shuffle').onclick = toggleShuffle;
$('#b-repeat').onclick = () => { P.repeat = { off: 'all', all: 'one', one: 'off' }[P.repeat]; syncControls(); saveStateSoon(); };
$('#b-mute').onclick = () => { if (P.vol) { P.prevVol = P.vol; P.vol = 0; } else P.vol = P.prevVol || 70; applyVolume(); lsSet('vinyl.vol', P.vol); };
$('#vol').oninput = e => { P.vol = +e.target.value; applyVolume(); lsSet('vinyl.vol', P.vol); };
$('#seek').addEventListener('input', e => { seeking = true; e.target.style.setProperty('--p', e.target.value / 10 + '%'); $('#t-cur').textContent = fmt(e.target.value / 1000 * (audio.duration || 0)); });
$('#seek').addEventListener('change', e => { if (audio.duration) audio.currentTime = e.target.value / 1000 * audio.duration; seeking = false; });
$('#b-queue').onclick = () => go(route.name === 'queue' ? '#/home' : '#/queue');
// Closed: open Now Playing with the lyrics showing. Open: just show or hide the lyrics and stay there.
$('#b-lyrics').onclick = () => { if (!S.np) { S.np = true; S.lyrics = true; } else S.lyrics = !S.lyrics; applyNPState(); };
$('#bar-track').addEventListener('click', e => { if (e.target.closest('.disc')) { S.np = !S.np; applyNPState(); } });
$('#nav-np').onclick = () => { S.np = !S.np; applyNPState(); };
$('#close-stage').onclick =() => { S.np = false; applyNPState(); };
$('#toggle-np').onclick = () => { S.np = !S.np; applyNPState(); };
function applyNPState() {
  document.body.classList.toggle('stage-open', S.np);
  document.body.classList.toggle('lyrics-off', !S.lyrics);
  $('#nav-np')?.classList.toggle('on', S.np);
  lsSet('vinyl.np', S.np); lsSet('vinyl.lyrics', S.lyrics); syncControls();
  if (S.np) { AsciiVinyl.start(() => !audio.paused); AsciiVinyl.setCover(P.cur?.cover); renderNP(); } else AsciiVinyl.stop();
}

function mediaSession() {
  if (!('mediaSession' in navigator) || !P.cur) return;
  const t = P.cur;
  navigator.mediaSession.metadata = new MediaMetadata({ title: t.title, artist: t.artist, artwork: t.cover ? [{ src: new URL(t.cover, location.origin).href, sizes: '600x600', type: 'image/jpeg' }] : [] });
  for (const [a, f] of [['play', () => audio.play()], ['pause', () => audio.pause()], ['nexttrack', () => next(false)], ['previoustrack', prev], ['stop', () => audio.pause()]]) { try { navigator.mediaSession.setActionHandler(a, f); } catch { /* unsupported */ } }
}

let saveTimer = 0;
function saveStateSoon(gap = 800) {
  if (saveTimer) return;
  saveTimer = setTimeout(() => {
    saveTimer = 0;
    const body = { version: 2, currentId: P.cur?.id || null, position: audio.currentTime || 0, queue: P.ctx.slice(0, 500), upNext: P.userQ, shuffle: P.shuffle, repeat: P.repeat, label: P.label };
    lsSet('vinyl.state', body);
    api('/api/state', { method: 'POST', body }).catch(() => { });
  }, gap);
}

function restoreFrom(st) {
  P.vol = lsGet('vinyl.vol', 70);
  if (st) {
    P.shuffle = !!st.shuffle; P.repeat = ['off', 'all', 'one'].includes(st.repeat) ? st.repeat : 'off';
    P.label = st.label || '';
    const ctx = (st.queue || []).filter(id => S.by.has(id));
    if (st.currentId && S.by.has(st.currentId)) {
      P.ctx = ctx.includes(st.currentId) ? ctx : [st.currentId, ...ctx];
      const cur = st.currentId;
      P.order = P.shuffle ? [cur, ...shuffled(P.ctx.filter(x => x !== cur))] : P.ctx.slice();
      P.pos = Math.max(0, P.order.indexOf(cur));
      P.userQ = (st.upNext || []).filter(id => S.by.has(id) && id !== cur);
      loadTrack(cur, false, st.position || 0);
    }
  }
  applyVolume(); syncControls();
}

/* ------------------------------------------------------------ player bar */

function heartBtn(id, extra = '') {
  const on = isLiked(id);
  return `<button class="heart ${on ? 'on' : ''} ${extra}" data-act="like" data-id="${esc(id)}" title="${on ? 'Remove from Liked Songs' : 'Save to Liked Songs'}" aria-label="${on ? 'Remove from Liked Songs' : 'Save to Liked Songs'}" aria-pressed="${on}">${ICON.heart}</button>`;
}
const artistLink = t => `<a href="#/artist/${enc(primaryArtist(t.artist))}">${esc(t.artist || 'Unknown artist')}</a>`;

function renderBarTrack() {
  const t = P.cur, el = $('#bar-track');
  if (!t) { el.innerHTML = '<div class="disc"><div class="label"></div></div><div class="txt"><div class="empty-note">Nothing playing</div></div>'; return; }
  const cover = t.cover ? `background-image:url('${esc(t.cover)}')` : `background:hsl(${hueOf(t)} 45% 34%)`;
  el.innerHTML = `<div class="disc" title="Now playing"><div class="label" style="${cover}"></div></div><div class="txt"><div class="tt">${esc(t.title)}</div><div class="ta">${t.isPv ? `<span class="newtag">DOWNLOADING</span>${esc(t.artist)}` : artistLink(t)}</div></div>${heartBtn(t.id)}`;
}
function syncHearts() {
  $$('.heart[data-id]').forEach(b => {
    const on = isLiked(b.dataset.id);
    b.classList.toggle('on', on); b.setAttribute('aria-pressed', on);
    b.title = on ? 'Remove from Liked Songs' : 'Save to Liked Songs';
  });
}
function markNow() {
  $$('.row').forEach(r => r.classList.toggle('now', r.dataset.id === P.cur?.id));
  $$('.lib-item').forEach(r => r.classList.toggle('now', !!r.dataset.playing && r.dataset.playing === P.cur?.id));
}

/* ------------------------------------------------------- now playing panel */

const lyricCache = new Map();
let lyricLines = [], lyricIdx = -1, lyricFor = null;

function renderNP() {
  const el = $('#np'); if (!S.np) return;
  const t = P.cur;
  if (!t) { el.innerHTML = `<div class="np-head">Now playing</div><p class="note">Play something and the record spins up here, with its lyrics.</p>`; return; }
  el.innerHTML = `
    <div class="np-head">${esc(P.label || 'Now playing')}</div>
    <div class="np-title"><div><div class="tt">${esc(t.title)}</div><div class="np-artist">${t.isPv ? esc(t.artist) + (t.item?.album ? ' · ' + esc(t.item.album) : '') : artistLink(t) + (t.album ? ` · <a href="${albumHref(t)}">${esc(t.album)}</a>` : '')}</div></div>${heartBtn(t.id)}</div>
    ${t.isPv ? `<div class="pv-note">Downloading this song to your library. It starts playing as soon as it is ready.</div>` : ''}
    ${t.isPv || !S.lyrics ? '' : `<div class="np-sec grow"><h4>Lyrics <button data-act="refresh-lyrics">Reload</button></h4><div id="lyr" class="lyr plain"><p class="note">Loading lyrics…</p></div></div>`}`;
  if (!t.isPv && S.lyrics) loadLyrics(t);
}

async function loadLyrics(t, refresh = false) {
  lyricFor = t.id; lyricLines = []; lyricIdx = -1;
  let res = !refresh && lyricCache.get(t.id);
  if (!res) {
    try { res = await api(`/api/tracks/${enc(t.id)}/lyrics${refresh ? '?refresh=1' : ''}`); lyricCache.set(t.id, res); }
    catch { res = { status: 'error' }; }
  }
  if (lyricFor !== t.id) return;
  const box = $('#lyr'); if (!box) return;
  if (res.status !== 'found') {
    const msg = { none: 'No lyrics found for this song.', instrumental: 'Instrumental.', disabled: 'Lyrics are turned off.', error: "Couldn't reach the lyrics service." }[res.status] || 'No lyrics.';
    box.innerHTML = `<p class="note">${msg}</p>`; return;
  }
  if (res.synced?.length) {
    lyricLines = res.synced;
    box.className = 'lyr';
    box.innerHTML = lyricLines.map((l, i) => `<p data-i="${i}" data-t="${l.t}">${esc(l.text) || '♪'}</p>`).join('');
    highlightLyric(audio.currentTime);
  } else {
    box.className = 'lyr plain';
    box.innerHTML = String(res.plain || '').split('\n').map(l => `<p>${esc(l) || '&nbsp;'}</p>`).join('');
  }
}
function highlightLyric(c) {
  if (!lyricLines.length) return;
  let lo = 0, hi = lyricLines.length - 1, i = -1;
  while (lo <= hi) { const m = (lo + hi) >> 1; if (lyricLines[m].t <= c + 0.15) { i = m; lo = m + 1; } else hi = m - 1; }
  if (i === lyricIdx) return;
  lyricIdx = i;
  const box = $('#lyr'); if (!box) return;
  [...box.children].forEach((p, j) => { p.classList.toggle('cur', j === i); p.classList.toggle('past', j < i); });
  const cur = box.children[i];
  if (cur) box.scrollTop = cur.offsetTop - box.clientHeight / 2 + cur.clientHeight / 2;
}

/* -------------------------------------------------------------- rendering */

function regList(ids) { const k = 'l' + Object.keys(S.lists).length; S.lists[k] = ids; return k; }

function rowHTML(id, i, key, { album = true, removeFrom = null, n = i + 1, queueSec = null } = {}) {
    const t = trackOf(id); if (!t) return '';
    return `<div class="row ${P.cur?.id === id ? 'now' : ''}" data-id="${esc(id)}" data-list="${key}" data-i="${i}">
      <div class="num"><span class="n">${n}</span><span class="eq"><i></i><i></i><i></i></span><svg class="p" viewBox="0 0 24 24"><path d="M7 4.5v15l13-7.5z"/></svg></div>
      <div class="title">${artHTML(t)}<div class="txt"><div class="tt">${esc(t.title)}</div><div class="ta">${artistLink(t)}</div></div></div>
      ${album ? `<div class="alb">${t.album ? `<a href="${albumHref(t)}">${esc(t.album)}</a>` : ''}</div>` : ''}
      ${heartBtn(id)}
      <div class="dur">${fmt(t.duration)}</div>
      ${queueSec
        ? `<button class="more qrm" data-act="queue-remove" data-sec="${queueSec}" data-i="${i}" title="Remove from queue" aria-label="Remove ${esc(t.title)} from the queue">${ICON.x}</button>`
        : `<button class="more" data-act="more" data-id="${esc(id)}" ${removeFrom ? `data-from="${esc(removeFrom)}"` : ''} aria-label="More options for ${esc(t.title)}">${ICON.more}</button>`}
    </div>`;
}
const webRowHTML = (i, n, key, idx) => `<div class="row web-row ${P.cur?.id === 'pv:' + i.id ? 'now' : ''}" data-id="pv:${esc(i.id)}" data-web="${esc(i.id)}" data-list="${key}" data-i="${idx}">
      <div class="num"><span class="n">${n}</span><span class="eq"><i></i><i></i><i></i></span><svg class="p" viewBox="0 0 24 24"><path d="M7 4.5v15l13-7.5z"/></svg></div>
      <div class="title"><div class="art" style="background-image:url('${esc(i.thumbnail)}')"></div><div class="txt"><div class="tt">${esc(i.title)}</div><div class="ta"><span class="newtag">NEW</span>${esc(i.artist)}${i.because ? ' · like ' + esc(i.because) : ''}</div></div></div>
      <div class="alb">${esc(i.album || 'New to your library')}</div><div></div><div class="dur">${fmt(i.duration)}</div>
      <button class="more" data-act="add-catalog" data-key="${esc(i.id)}" title="Add to library" aria-label="Add ${esc(i.title)} to library">${ICON.plus}</button></div>`;
/* A list of not-yet-owned songs: playable as previews, each with an Add button. */
function previewRows(items) {
  const ids = items.map(registerPv), key = regList(ids);
  return `<div class="tl">${tlHead(true)}${items.map((it, i) => webRowHTML(it, i + 1, key, i)).join('')}</div>`;
}
const tlHead = album => `<div class="tl-head"><div style="text-align:center">#</div><div>Title</div>${album ? '<div class="alb">Album</div>' : ''}<div></div><div style="text-align:right">Time</div><div></div></div>`;
function trackRows(ids, { album = true, removeFrom = null, sort = '', queueSec = null } = {}) {
  const key = regList(ids);
  const rows = ids.map((id, i) => rowHTML(id, i, key, { album, removeFrom, queueSec })).join('');
  return `<div class="tl ${album ? '' : 'no-album'}" ${sort ? `data-sort="${esc(sort)}"` : ''}><div class="tl-head"><div style="text-align:center">#</div><div>Title</div>${album ? '<div class="alb">Album</div>' : ''}<div></div><div style="text-align:right">Time</div><div></div></div>${rows}</div>`;
}

function card({ kind, key, art, title, sub, round = false, href = '', playKey = '' }) {
  const attrs = href ? `href="${href}"` : '';
  return `<div class="card" data-kind="${kind}" data-key="${esc(key)}" data-href="${esc(href)}" tabindex="0" role="link" aria-label="${esc(title)}">
    <div class="artw">${art.replace('class="art ', `class="art ${round ? 'round ' : ''}`)}${playKey ? `<button class="go" data-act="play-card" data-play="${esc(playKey)}" aria-label="Play ${esc(title)}">${ICON.play}</button>` : ''}</div>
    <div class="t">${esc(title)}</div><div class="s">${esc(sub || '')}</div></div>`;
}
const trackCard = id => { const t = trackOf(id); return t ? card({ kind: 'track', key: id, art: artHTML(t), title: t.title, sub: t.artist, playKey: 'track:' + id, href: '' }) : ''; };
const artistCard = k => {
  const T = taste(), ts = (T.byArtist.get(k) || []).slice().sort((a, b) => (b.plays || 0) - (a.plays || 0));
  return card({ kind: 'artist', key: k, art: artHTML(ts[0]), title: T.names.get(k) || k, sub: 'Artist', round: true, playKey: 'artist:' + k, href: `#/artist/${enc(T.names.get(k) || k)}` });
};
const playlistCard = pl => card({ kind: 'playlist', key: pl.id, art: playlistArt(pl), title: pl.name, sub: `${plural((pl.track_ids || []).length, 'song')}`, playKey: 'playlist:' + pl.id, href: `#/playlist/${pl.id}` });
const mixCard = m => card({ kind: 'mix', key: String(m.n), art: mosaicHTML(m.ids), title: m.name, sub: m.artists.join(', ') + ' and more' + (mixNews(m).length ? ` · ${mixNews(m).length} new` : ''), playKey: 'mix:' + m.n, href: `#/mix/${m.n}` });
function playlistArt(pl, cls = '') {
  if (pl.cover) return `<div class="art ${cls}" style="background-image:url('${esc(pl.cover)}')"></div>`;
  return mosaicHTML(pl.track_ids || [], cls);
}

let shelfN = 0;
function shelf(title, sub, cards, id, cls = '') {
  if (!cards.length) return '';
  shelfN++;
  return `<section class="shelf ${cls}"><div class="shelf-head"><div><h2><i>${String(shelfN).padStart(2, '0')}</i>${esc(title)}</h2>${sub ? `<div class="sub">${esc(sub)}</div>` : ''}</div>${cls.includes('scroll')
    ? `<div class="arrows"><button data-act="scroll-shelf" data-dir="-1" aria-label="Scroll left">&#8249;</button><button data-act="scroll-shelf" data-dir="1" aria-label="Scroll right">&#8250;</button></div>`
    : `<button data-act="toggle-shelf" aria-expanded="false">Show all</button>`}</div><div class="grid" ${id ? `id="${id}"` : ''}>${cards.join('')}</div></section>`;
}


function qtile(inner, playKey, href) {
  return `<div class="qtile" data-href="${esc(href)}" tabindex="0" role="link">${inner}<button class="go" data-act="play-card" data-play="${esc(playKey)}" aria-label="Play">${ICON.play}</button></div>`;
}

function renderHome() {
  const view = $('#view');
  if (S.tracks.length < 5) { renderStart(); return; }
  const sh = shelves(), T = taste();
  const tiles = [qtile(`${likedArt('')}<div class="t">Liked Songs</div>`, 'liked', '#/liked')];
  for (const m of sh.mixes) tiles.push(qtile(`${mosaicHTML(m.ids)}<div class="t">${esc(m.name)}</div>`, 'mix:' + m.n, `#/mix/${m.n}`));
  for (const pl of S.playlists.slice(0, 8 - tiles.length)) tiles.push(qtile(`${playlistArt(pl)}<div class="t">${esc(pl.name)}</div>`, 'playlist:' + pl.id, `#/playlist/${pl.id}`));
  for (const k of sh.topArtists.slice(0, 8 - tiles.length)) { const ts = T.byArtist.get(k) || []; tiles.push(qtile(`${artHTML(ts[0])}<div class="t">${esc(T.names.get(k))}</div>`, 'artist:' + k, `#/artist/${enc(T.names.get(k))}`)); }
  const hasTaste = S.history.length > 0 || S.likes.size > 0 || S.tracks.some(t => t.plays);
  const again = sh.recent.length ? shelf('Pick up where you left off', '', sh.recent.slice(0, 6).map(trackCard)) : shelf('Recently added', '', sh.added.slice(0, 6).map(trackCard));
  view.innerHTML = `<h1 class="big">Home</h1>
    ${shelf('Made for You', '', sh.mixes.map(mixCard), '', 'picks')}
    <div id="web-recs"></div>`;
  tint(P.cur ? [P.cur.id] : sh.recent.slice(0, 1));
  ensureMixNews();
  loadWebRecs();
}

/* ---- new music: Deezer related artists + genres through /api/discover ---- */
const normTitle = x => String(x || '').toLowerCase().replace(/[\(\[][^\)\]]*[\)\]]/g, '').replace(/\s-\s.*$/, '').replace(/[^a-z0-9]/g, '');
let ownedSet = { ver: -1, set: new Set() };
function owned(i) {
  if (ownedSet.ver !== S.ver) ownedSet = { ver: S.ver, set: new Set(S.tracks.map(t => normTitle(t.title))) };
  return i.in_library || ownedSet.set.has(normTitle(i.title));
}
async function dz(path) {
  const k = 'dz:' + path, hit = S.web.get(k);
  if (hit && Date.now() - hit.at < 15 * 60000) return hit.res;
  const res = await api('/api/discover/' + path);
  if (res.complete !== false) S.web.set(k, { at: Date.now(), res });
  return res;
}
const similarFor = (name, n = 6, per = 2) => dz(`similar?artist=${enc(name)}&n=${n}&per=${per}`).catch(() => null);
function roundRobin(lists, n) {
  const seen = new Set(), out = [];
  for (let i = 0; i < 12 && out.length < n; i++) for (const arr of lists) { const it = arr[i]; if (it && !seen.has(it.id) && out.length < n) { seen.add(it.id); out.push(it); } }
  return out;
}
/* Related artists -> songs you don't own, artists you don't know first, one artist at a time. */
function freshFrom(results, n) {
  const known = taste().byArtist, lists = [];
  for (const res of results) for (const a of res?.related || []) {
    const items = (a.tracks || []).filter(t => !owned(t));
    if (items.length) lists.push({ items, fresh: !known.has(a.name.toLowerCase()) });
  }
  lists.sort((a, b) => b.fresh - a.fresh);
  return stash(roundRobin(lists.map(l => l.items), n));
}
const mixKey = m => m.name + '|' + m.artists.join();
S.mixNews = lsGet('vinyl.mixNews', {});
let mixNewsBusy = false;
async function ensureMixNews() {
  if (mixNewsBusy) return; mixNewsBusy = true;
  let changed = false;
  try {
    for (const m of mixes()) {
      const key = mixKey(m); if (S.mixNews[key]) continue;
      const res = await Promise.all(m.artists.slice(0, 3).map(a => similarFor(a, 5, 2)));
      const items = freshFrom(res, 10);
      if (!items.length && res.some(r => !r || r.complete === false)) continue; // try again next time
      S.mixNews[key] = items;
      lsSet('vinyl.mixNews', S.mixNews);
      changed = true;
    }
  } finally { mixNewsBusy = false; }
  if (!changed) return;
  if (route.name === 'mix') renderRoute(true);
  else if (route.name === 'home') for (const m of mixes()) { // update the mix cards in place: no re-render
    const sub = $(`.card[data-kind="mix"][data-key="${m.n}"] .s`);
    if (sub && mixNews(m).length) sub.textContent = m.artists.join(', ') + ` and more · ${mixNews(m).length} new`;
  }
}
const mixNews = m => S.mixNews[mixKey(m)] || [];
/* Library songs with a new song slipped in after every third. */
function mixEntries(m) {
  const news = mixNews(m), out = []; let ni = 0;
  m.ids.forEach((id, i) => { out.push({ id }); if ((i + 1) % 3 === 0 && news[ni]) out.push({ web: news[ni++] }); });
  return out;
}
const mixIds = m => mixEntries(m).map(e => e.id || registerPv(e.web));

/* "New music for you" is computed once and kept for a while, so the shelf never reshuffles under you. */
let recsLoading = null;
const recsIds = r => r.items.filter(i => !owned(i)).map(i => i.id).join('|');
function recsShelf(r) {
  const items = r.items.filter(i => !owned(i));
  return items.length ? shelf('New Music', `From artists like ${r.seeds.join(', ')}. Click to download and play.`, items.map(webCard), '', 'scroll') : '';
}
async function loadWebRecs() {
  const box = $('#web-recs'); if (!box) return;
  if (S.recs?.items) stash(S.recs.items);
  const fresh = S.recs && Date.now() - S.recs.at < 20 * 60000;
  if (S.recs?.items?.length) { box.innerHTML = recsShelf(S.recs); box.dataset.ids = recsIds(S.recs); } // last time's picks, instantly
  else box.style.minHeight = '340px'; // hold the space so nothing jumps when it arrives
  if (fresh) { box.style.minHeight = ''; return; }
  if (!recsLoading) {
    recsLoading = (async () => {
      const T = taste(), seeds = T.top.slice(0, 3).map(k => T.names.get(k)).filter(Boolean);
      if (!seeds.length) return;
      const res = await Promise.all(seeds.map(a => similarFor(a, 6, 2)));
      const items = freshFrom(res, 12);
      if (items.length || !res.some(r => !r || r.complete === false)) { S.recs = { at: Date.now(), seeds, items }; lsSet('vinyl.recs', S.recs); }
    })().finally(() => { recsLoading = null; });
  }
  await recsLoading;
  const now = $('#web-recs'); if (!now) return;
  now.style.minHeight = '';
  // only redraw if the picks really changed, so a refresh in the background doesn't flicker
  if (S.recs?.items.length && ['home', 'add', 'discover'].includes(route.name) && now.dataset.ids !== recsIds(S.recs)) {
    now.innerHTML = recsShelf(S.recs); now.dataset.ids = recsIds(S.recs);
  }
}
const webCard = i => `<div class="card" data-kind="web" data-key="${esc(i.id)}"><div class="artw"><div class="art " style="background-image:url('${esc(i.thumbnail)}')"></div><button class="go" data-act="play-pv" data-key="${esc(i.id)}" aria-label="Play ${esc(i.title)}">${ICON.play}</button></div><div class="t">${esc(i.title)}</div><div class="s"><span class="tagnew">NEW</span> ${esc(i.artist)}${i.because ? ' · like ' + esc(i.because) : ''}</div><button class="add-btn" style="margin-top:8px;min-width:0;padding:4px 10px" data-act="add-catalog" data-key="${esc(i.id)}">+ Add</button></div>`;
const dzArtistCard = a => card({ kind: 'dz', key: a.id, art: `<div class="art " style="background-image:url('${esc(a.picture)}')"></div>`, title: a.name, sub: a.fans ? `${Math.round(a.fans / 1000).toLocaleString()}k fans` : 'Artist', round: true, href: `#/dz/${a.id}` });

async function playPv(key, el) {
  const grid = el?.closest('.grid'), keys = grid ? $$('.card[data-kind="web"]', grid).map(c => c.dataset.key) : [key];
  const ids = keys.map(k => S.webItems[k]).filter(Boolean).map(registerPv);
  setContext(ids, Math.max(0, keys.indexOf(key)), { label: 'New music' });
}
function replacePv(pid, real) {
  for (const arr of [P.order, P.ctx, P.userQ]) { const i = arr.indexOf(pid); if (i >= 0) arr[i] = real; }
}
const downloads = new Map();
/* Downloads a discovery song (once, even if asked twice); resolves to the library id or null. */
function downloadPv(t) {
  if (downloads.has(t.id)) return downloads.get(t.id);
  const it = t.item;
  const p = (async () => {
    if (!it) return null;
    const job = await startJob(`${it.title} — ${it.artist}`, { mode: 'catalog', catalog: it.catalog }, { quiet: true });
    if (job.status === 'done' && job.fileId && S.by.has(job.fileId)) { replacePv(t.id, job.fileId); return job.fileId; }
    return null;
  })().finally(() => downloads.delete(t.id));
  downloads.set(t.id, p);
  return p;
}
async function addPv(t) {
  toast(`Adding ${t.title}…`);
  const real = await downloadPv(t);
  if (real) { toast(`Added ${t.title}`); renderRoute(true); }
}

function heroHTML({ kind, title, art, meta, round }) {
  return `<div class="hero">${art.replace('class="art ', `class="art ${round ? 'round ' : ''}`)}<div><div class="kind">${esc(kind)}</div><h1 class="big">${esc(title)}</h1><div class="meta">${meta}</div></div></div>`;
}
function actionsHTML(playKey, { menu = '' } = {}) {
  const more = ['album', 'mix', 'artist', 'liked'].includes(playKey.split(':')[0])
    ? `<button class="ghost-btn" data-act="source-menu" data-play="${esc(playKey)}" title="More options" aria-label="More options">${ICON.more}</button>` : '';
  return `<div class="actions"><button class="go lg" data-act="play-card" data-play="${esc(playKey)}" aria-label="Play">${ICON.play}</button><button class="ghost-btn ${P.shuffle ? 'on' : ''}" data-act="shuffle-card" data-play="${esc(playKey)}" title="Shuffle play" aria-label="Shuffle play">${ICON.shuffle}</button>${more}${menu}</div>`;
}
const totalDur = ids => ids.reduce((s, id) => s + (trackOf(id)?.duration || 0), 0);

/* ---- albums: songs grouped by artist + album name (names come from catalog downloads and a background lookup) ---- */
const albumKey = t => akey(t) + '||' + t.album.toLowerCase();
const albumHref = t => `#/album/${enc(albumKey(t))}`;
function albums() {
  if (S.cache.albums?.ver === S.ver) return S.cache.albums.list;
  const m = new Map();
  for (const t of S.tracks) {
    if (!t.album) continue;
    const k = albumKey(t);
    (m.get(k) || m.set(k, { key: k, name: t.album, artist: primaryArtist(t.artist), tracks: [] }).get(k)).tracks.push(t);
  }
  const list = [...m.values()].sort((a, b) => a.name.localeCompare(b.name));
  S.cache.albums = { ver: S.ver, list };
  return list;
}
const albumArt = a => artHTML(a.tracks.find(t => t.cover) || a.tracks[0]);
const albumCard = a => card({ kind: 'album', key: a.key, art: albumArt(a), title: a.name, sub: `${a.artist} · ${plural(a.tracks.length, 'song')}`, playKey: 'album:' + a.key, href: `#/album/${enc(a.key)}` });

function sourceIds(key) {
  const [kind, ...rest] = key.split(':'), arg = rest.join(':');
  switch (kind) {
    case 'liked': return [...S.likes.entries()].sort((a, b) => b[1] - a[1]).map(([id]) => id);
    case 'playlist': return (S.playlists.find(p => p.id === arg)?.track_ids || []).filter(id => S.by.has(id));
    case 'mix': { const m = mixes().find(x => String(x.n) === arg); return m ? mixIds(m) : []; }
    case 'artist': return (taste().byArtist.get(arg) || []).slice().sort((a, b) => (b.plays || 0) - (a.plays || 0) || a.title.localeCompare(b.title)).map(t => t.id);
    case 'album': return (albums().find(a => a.key === arg)?.tracks || []).slice().sort((x, y) => x.title.localeCompare(y.title)).map(t => t.id);
    case 'track': return [arg];
    case 'all': return S.tracks.map(t => t.id);
    default: return [];
  }
}
function sourceLabel(key) {
  const [kind, ...rest] = key.split(':'), arg = rest.join(':');
  if (kind === 'liked') return 'Liked Songs';
  if (kind === 'album') return albums().find(a => a.key === arg)?.name || 'Album';
  if (kind === 'playlist') return S.playlists.find(p => p.id === arg)?.name || 'Playlist';
  if (kind === 'mix') return mixes().find(m => String(m.n) === arg)?.name || 'Mix';
  if (kind === 'artist') return taste().names.get(arg) || 'Artist';
  if (kind === 'track') { const t = trackOf(arg); return t ? `${t.title} radio` : ''; }
  return 'Your library';
}
function playSource(key, { shuffle = false } = {}) {
  let ids = sourceIds(key);
  if (!ids.length) { toast('Nothing to play yet'); return; }
  setContext(ids, 0, { shuffle: shuffle || P.shuffle, label: sourceLabel(key) });
  if (key.startsWith('mix:')) { const m = mixes().find(x => String(x.n) === key.slice(4)); if (m) autoAddNews(mixNews(m).slice(0, 5), P.label); }
}

function renderLiked() {
  const ids = sourceIds('liked');
  $('#view').innerHTML = `${heroHTML({ kind: 'Playlist', title: 'Liked Songs', art: likedArt(''), meta: `<b>${plural(ids.length, 'song')}</b>${ids.length ? ` · ${fmtLong(totalDur(ids))}` : ''}` })}
    ${ids.length ? actionsHTML('liked') + trackRows(ids) : `<div class="empty"><b>Songs you like will appear here</b>Tap the heart on any song to save it.</div>`}`;
  $('#view').style.setProperty('--hue', '#7a3418');
}
/* ---- app updates (the server asks GitHub for the latest release) ---- */
let updateChecking = false;
async function checkForUpdate({ manual = false } = {}) {
  if (updateChecking) return null; // already looking: extra clicks don't start more checks, toasts or dialogs
  updateChecking = true;
  try { return await checkForUpdate_({ manual }); } finally { updateChecking = false; }
}
async function checkForUpdate_({ manual }) {
  let info;
  try { info = await api('/api/update'); }
  catch (e) { if (manual) toast(e.message); return null; }
  S.update = info;
  if (!info.available) { if (manual) toast(`You're up to date (Vinyl ${info.current})`); return info; }
  const notes = String(info.notes || '').replace(/\*\*|__|`/g, '').split('\n').filter(l => l.trim() && !/^full changelog/i.test(l.trim())).slice(0, 8).join('\n');
  const yes = await modal({ title: 'Update available', text: `Vinyl ${info.latest} is ready. You have ${info.current}.${notes ? '\n\n' + notes : ''}`, ok: 'Install and restart', cancel: 'Not now' });
  if (yes) installUpdate();
  return info;
}
async function installUpdate() {
  const w = $('#modal');
  const show = (msg, pct, closable) => {
    w.hidden = false;
    w.innerHTML = `<div class="modal"><h3>Updating Vinyl</h3><p>${esc(msg)}</p>${closable ? '<div class="btns"><button class="pill-btn" data-x="close">Close</button></div>' : `<div class="bar"><i style="width:${pct}%"></i></div>`}</div>`;
    if (closable) $('[data-x]', w).onclick = () => { w.hidden = true; w.innerHTML = ''; };
  };
  show('Starting the download…', 0);
  try { await api('/api/update/install', { method: 'POST' }); } catch (e) { show(e.message, 0, true); return; }
  for (;;) {
    await new Promise(r => setTimeout(r, 500));
    let st;
    try { st = await api('/api/update/status'); } catch { show('Restarting Vinyl…', 100); return; } // the app has exited for the installer
    if (st.status === 'downloading') show(st.total ? `Downloading… ${Math.round(st.done / st.total * 100)}%` : 'Downloading…', st.total ? st.done / st.total * 100 : 5);
    else if (st.status === 'launching') show('Installing. Vinyl will reopen in a moment.', 100);
    else if (st.status === 'error') { show(st.error || 'The update failed.', 0, true); return; }
  }
}

function renderSettings() {
  $('#view').innerHTML = `<h1 class="big">Settings</h1>
    <div class="statline"><b>${S.tracks.length}</b> songs · <b>${S.playlists.length}</b> playlists · <b>${S.likes.size}</b> liked</div>
    <section class="shelf"><div class="shelf-head"><div><h2>Updates</h2><div class="sub">Vinyl checks for a new version each time it starts</div></div></div>
      <div class="set-row"><div><b>Version</b><div class="note" id="upd-ver">Checking…</div></div><button class="pill-btn ghost" data-act="check-update">Check for updates</button></div>
    </section>
    <section class="shelf"><div class="shelf-head"><div><h2>Download tools</h2><div class="sub">Vinyl uses yt-dlp to download songs and ffmpeg to convert them</div></div></div>
      <div class="set-row"><div><b>Status</b><div class="note" id="deps-status">Checking…</div></div><button class="pill-btn ghost" id="deps-btn" data-act="install-deps" hidden>Install missing tools</button></div>
    </section>
    <section class="shelf"><div class="shelf-head"><div><h2>Your data</h2><div class="sub">Everything lives in the data and library folders next to the app</div></div></div>
      <div class="set-row"><div><b>Refresh data files</b><div class="note">Rewrites the saved JSON files (settings, playlists, lyrics and more) in the readable, indented format. Nothing is deleted.</div></div><button class="pill-btn ghost" data-act="refresh-json">Refresh</button></div>
      <div class="set-row"><div><b>Reset recommendations</b><div class="note">Clears liked songs, listening history and play counts. Your songs and playlists stay.</div></div><button class="pill-btn ghost" data-act="reset-recs">Reset</button></div>
      <div class="set-row"><div><b>Delete all playlists</b><div class="note">Removes every playlist. Songs stay in the library.</div></div><button class="pill-btn ghost" data-act="delete-playlists">Delete</button></div>
      <div class="set-row"><div><b>Delete everything</b><div class="note">Deletes every song file, playlist, like and the listening history. This cannot be undone.</div></div><button class="pill-btn danger" data-act="delete-all">Delete everything</button></div>
    </section>`;
  $('#view').style.setProperty('--hue', '#1a120d');
  refreshDeps();
  api('/api/update/status').then(st => { const el = $('#upd-ver'); if (el) el.textContent = `Vinyl ${st.current}${S.update ? (S.update.available ? ` · ${S.update.latest} is available` : ' · up to date') : ''}`; }).catch(() => { });
}
async function wipe(kind) {
  const cfg = {
    recs: { title: 'Reset recommendations?', text: 'Liked songs, listening history and play counts will be cleared.', ok: 'Reset' },
    playlists: { title: 'Delete all playlists?', text: 'Every playlist is removed. Your songs stay.', ok: 'Delete' },
    all: { title: 'Delete everything?', text: 'Every song file, playlist, like and your listening history will be permanently deleted. This cannot be undone.', ok: 'Delete everything' },
  }[kind];
  if (!await modal({ ...cfg, danger: true })) return;
  try {
    if (kind === 'recs') await api('/api/history', { method: 'DELETE' });
    else if (kind === 'playlists') await api('/api/playlists', { method: 'DELETE' });
    else { audio.pause(); audio.removeAttribute('src'); P.cur = null; P.ctx = []; P.order = []; P.userQ = []; P.pos = -1; await api('/api/tracks', { method: 'DELETE' }); await api('/api/playlists', { method: 'DELETE' }); await api('/api/history', { method: 'DELETE' }); renderBarTrack(); renderNP(); }
  } catch (e) { toast(e.message); return; }
  S.cache = {}; S.mixNews = {}; S.recs = null;
  await loadAll(); renderRoute(true); toast('Done');
}

function renderPlaylists() {
  $('#view').innerHTML = `<h1 class="big">Playlists</h1>
    <div class="actions" style="margin-top:18px"><button class="pill-btn" data-act="new-playlist">New playlist</button><button class="pill-btn ghost" data-act="artist-playlists">From artists</button></div>
    ${S.playlists.length ? `<div class="grid open">${S.playlists.map(playlistCard).join('')}</div>` : '<div class="empty"><b>No playlists yet</b>Create one, then add songs from the ••• menu on any song.</div>'}`;
  $('#view').style.setProperty('--hue', '#1a120d');
}

function renderPlaylist(id) {
  const pl = S.playlists.find(p => p.id === id);
  if (!pl) { $('#view').innerHTML = `<div class="empty"><b>Playlist not found</b></div>`; return; }
  const ids = sourceIds('playlist:' + id);
  const menu = `<button class="ghost-btn" data-act="playlist-menu" data-id="${esc(id)}" aria-label="More options">${ICON.more}</button><button class="pill-btn ghost" data-act="add-songs" data-pl="${esc(id)}">+ Add songs</button>`;
  let recs = '';
  if (ids.length) {
    const seeds = new Map();
    for (const x of ids) { const k = akey(trackOf(x)); seeds.set(k, (seeds.get(k) || 0) + 1); }
    const r = spread(scoreTracks(new Map([...seeds].map(([k, n]) => [k, Math.min(1, n / 2)])), { exclude: new Set(ids), novelty: 0.1 }), 6);
    recs = r.length ? `<div class="sect-title">Recommended songs</div><div class="note" style="margin-bottom:8px">Based on what is in this playlist</div>${recRows(r.map(t => t.id), id)}` : '';
  }
  const cover = `<div class="cover-wrap">${playlistArt(pl)}<button class="cover-btn" data-act="pl-cover" data-id="${esc(id)}">${ICON.image}<span>Change cover</span></button></div>`;
  $('#view').innerHTML = `${heroHTML({ kind: 'Playlist', title: pl.name, art: cover, meta: `<b>${plural(ids.length, 'song')}</b>${ids.length ? ` · ${fmtLong(totalDur(ids))}` : ''}` })}
    ${ids.length ? actionsHTML('playlist:' + id, { menu }) + trackRows(ids, { removeFrom: id, sort: 'playlist:' + id }) : `<div class="actions">${menu}</div><div class="empty"><b>This playlist is empty</b>Add songs from any song's ••• menu, or pick from the recommendations below.</div>`}`;
  tint(ids);
}
function recRows(ids, pid) {
  return ids.map(id => { const t = trackOf(id); return `<div class="wrow">${artHTML(t)}<div style="min-width:0"><div class="tt">${esc(t.title)}</div><div class="ta">${esc(t.artist)}</div></div><button class="add-btn" data-act="add-to-pl" data-pid="${esc(pid)}" data-id="${esc(id)}">Add</button></div>`; }).join('');
}

function renderMix(n) {
  const m = mixes().find(x => String(x.n) === String(n));
  if (!m) { $('#view').innerHTML = `<div class="empty"><b>Mix not found</b>Play a few songs and it will be rebuilt.</div>`; return; }
  const entries = mixEntries(m), news = mixNews(m), ids = mixIds(m), key = regList(ids);
  const rows = entries.map((e, i) => e.web ? webRowHTML(e.web, i + 1, key, i) : rowHTML(e.id, i, key, { n: i + 1 })).join('');
  const addAll = news.length ? `<button class="pill-btn ghost" data-act="add-all-new" data-n="${m.n}">Add ${news.length} new songs</button>` : '';
  $('#view').innerHTML = `<div class="kicker">Made for you</div>${heroHTML({ kind: '', title: m.name, art: mosaicHTML(m.ids), meta: `<b>${esc(m.artists.join(', '))}</b> and more · ${plural(m.ids.length, 'song')}${news.length ? ` · <b>${news.length} new</b>` : ''}` })}${actionsHTML('mix:' + n, { menu: addAll })}
    <div class="tl">${tlHead(true)}${rows}</div>
    ${news.length ? '<div class="note" style="margin-top:14px">NEW songs come from artists related to this mix and play as 30 second previews. Playing the mix also adds the first five to your library in the background.</div>' : '<div class="note" style="margin-top:14px">Finding new songs from related artists…</div>'}`;
  tint(m.ids);
  ensureMixNews();
}

/* Adds new songs one at a time; each preview in the queue is swapped for the full song when it lands. */
async function autoAddNews(items, label) {
  for (const it of items) {
    if (label && P.label !== label) return;
    if (owned(it)) continue;
    const pv = S.pv.get('pv:' + it.id) || S.pv.get(registerPv(it));
    if (await downloadPv(pv)) toast(`Added to library: ${it.title}`);
  }
}

function renderAlbums() {
  const list = albums().slice().sort((a, b) => a.artist.localeCompare(b.artist) || a.name.localeCompare(b.name));
  const unnamed = S.tracks.filter(t => !t.album).length;
  $('#view').innerHTML = `<h1 class="big">Albums</h1>
    <div class="statline">${list.length ? `<b>${list.length}</b> albums` : 'No album names yet'}${unnamed ? ` · looking up names for ${plural(unnamed, 'song')} in the background` : ''}</div>
    ${list.length ? `<div class="grid open" style="margin-top:22px">${list.map(albumCard).join('')}</div>` : '<div class="empty"><b>Albums will appear here</b>Vinyl looks up album names for your songs in the background. Check back in a minute.</div>'}`;
  $('#view').style.setProperty('--hue', '#3a2418');
}

function renderAlbum(key) {
  const a = albums().find(x => x.key === key);
  if (!a) { $('#view').innerHTML = `<div class="empty"><b>Album not found</b></div>`; return; }
  const ids = sourceIds('album:' + key);
  $('#view').innerHTML = `${heroHTML({ kind: 'Album', title: a.name, art: albumArt(a), meta: `<a href="#/artist/${enc(a.artist)}"><b>${esc(a.artist)}</b></a> · ${plural(ids.length, 'song')} · ${fmtLong(totalDur(ids))}` })}
    ${actionsHTML('album:' + key)}${trackRows(ids, { album: false })}`;
  tint(ids);
}

function renderArtist(name) {
  const k = name.toLowerCase(), T = taste(), ids = sourceIds('artist:' + k);
  if (!ids.length) { $('#view').innerHTML = `<div class="empty"><b>No songs by ${esc(name)}</b></div>`; return; }
  const plays = ids.reduce((s, id) => s + (trackOf(id).plays || 0), 0);
  const rel = relatedArtists(k, 8).filter(x => T.byArtist.has(x));
  const fill = rel.length ? rel : T.top.filter(x => x !== k).slice(0, 6);
  $('#view').innerHTML = `${heroHTML({ kind: 'Artist', title: T.names.get(k) || name, art: artHTML(trackOf(ids[0])), round: true, meta: `<b>${plural(ids.length, 'song')}</b>${plays ? ` · ${plural(plays, 'play')}` : ''}` })}
    ${actionsHTML('artist:' + k)}
    <div class="sect-title" style="margin-top:6px">Popular</div>${trackRows(ids, { album: true })}`;
  tint(ids);
}

function renderLibrary_() {
  const sorts = { added: 'Recently added', title: 'Title', artist: 'Artist', plays: 'Most played' };
  let ts = S.tracks.slice();
  ts.sort({ added: (a, b) => b.added - a.added, title: (a, b) => a.title.localeCompare(b.title), artist: (a, b) => (a.artist || '').localeCompare(b.artist || '') || a.title.localeCompare(b.title), plays: (a, b) => (b.plays || 0) - (a.plays || 0) }[S.sort]);
  const ids = ts.map(t => t.id);
  $('#view').innerHTML = `<h1 class="big">Your songs</h1><div class="meta note" style="margin-top:6px">${plural(ids.length, 'song')} · ${fmtLong(totalDur(ids))}</div>
    <div class="actions" style="margin-top:10px"><button class="go lg" data-act="play-ids" data-key="all" aria-label="Play all">${ICON.play}</button>
    <button class="pill-btn" data-act="add-songs">+ Add songs</button>
    ${Object.entries(sorts).map(([k, v]) => `<button class="chip ${S.sort === k ? 'on' : ''}" data-act="sort" data-sort="${k}">${v}</button>`).join('')}</div>
    <div class="note" style="margin-bottom:6px">Drag songs or folders in from your computer, or drag a song onto a playlist in the sidebar. Ctrl or Shift click selects several. <a href="#/add" style="text-decoration:underline">Add from a link</a></div>
    ${ids.length ? trackRows(ids) : `<div class="empty"><b>No songs yet</b><div class="drop" style="justify-content:center"><button class="pill-btn" data-act="add-songs">Add songs from this computer</button><button class="pill-btn ghost" data-act="goto-add">Add from a link</button></div></div>`}`;
  $('#view').style.setProperty('--hue', '#3a2a22');
}

function renderSearch() {
  const q = S.q.trim(), view = $('#view');
  if (!q) {
    const T = taste(), cols = ['#e8115b', '#1e3264', '#8d67ab', '#e13300', '#148a08', '#dc148c', '#477d95', '#af2896', '#503750', '#006450'];
    view.innerHTML = `<h1 class="big">Search</h1><div class="note" style="margin-top:6px">Find songs, artists and playlists in your library, or add new music from the catalog.</div>
      ${T.top.length || S.tracks.length ? `<div class="sect-title">Browse your artists</div><div class="browse">${[...T.byArtist.keys()].slice(0, 24).map((k, i) => `<a class="tile" style="background:${cols[hash32(k) % cols.length]}" href="#/artist/${enc(T.names.get(k))}">${esc(T.names.get(k))}${artHTML((T.byArtist.get(k) || [])[0])}</a>`).join('')}</div>` : ''}`;
    view.style.setProperty('--hue', '#2d2a3a');
    return;
  }
  const lq = q.toLowerCase(), words = lq.split(/\s+/);
  const hit = t => { const h = `${t.title} ${t.artist} ${t.album || ''}`.toLowerCase(); return words.every(w => h.includes(w)); };
  const songs = S.tracks.filter(hit).sort((a, b) => (b.plays || 0) - (a.plays || 0));
  const T = taste(), artists = [...T.byArtist.keys()].filter(k => words.every(w => (T.names.get(k) || '').toLowerCase().includes(w)));
  const pls = S.playlists.filter(p => words.every(w => p.name.toLowerCase().includes(w)));
  let top = '';
  if (artists.length && (T.names.get(artists[0]).toLowerCase().startsWith(lq) || !songs.length)) {
    const k = artists[0], t0 = T.byArtist.get(k)[0];
    top = `<div class="tr-card" data-href="#/artist/${enc(T.names.get(k))}" tabindex="0" role="link">${artHTML(t0, 'round')}<div class="t">${esc(T.names.get(k))}</div><div class="s">Artist</div><button class="go" data-act="play-card" data-play="artist:${esc(k)}" aria-label="Play">${ICON.play}</button></div>`;
  } else if (songs.length) {
    const t = songs[0];
    top = `<div class="tr-card" data-id="${esc(t.id)}" data-act="play-one" tabindex="0">${artHTML(t)}<div class="t">${esc(t.title)}</div><div class="s">${esc(t.artist)}<span class="tag">Song</span></div><button class="go" data-act="play-one" data-id="${esc(t.id)}" aria-label="Play">${ICON.play}</button></div>`;
  }
  view.innerHTML = `${songs.length || artists.length || pls.length ? `<div class="top-result"><div><div class="sect-title" style="margin-top:6px">Top result</div>${top}</div>
      <div><div class="sect-title" style="margin-top:6px">Songs</div>${songs.length ? trackRows(songs.slice(0, 5).map(t => t.id), { album: false }).replace('<div class="tl-head">', '<div class="tl-head" hidden>') : '<div class="note">No matching songs in your library.</div>'}</div></div>` : `<div class="empty"><b>Nothing in your library for "${esc(q)}"</b>Try the catalog results below.</div>`}
    ${songs.length > 5 ? `<div class="sect-title">More songs</div>` + trackRows(songs.slice(5, 30).map(t => t.id)) : ''}
    ${artists.length ? shelf('Artists', '', artists.slice(0, 12).map(artistCard)) : ''}
    ${(() => { const als = albums().filter(a => words.every(w => `${a.name} ${a.artist}`.toLowerCase().includes(w))); return als.length ? shelf('Albums', '', als.slice(0, 12).map(albumCard)) : ''; })()}
    ${pls.length ? shelf('Playlists', '', pls.map(playlistCard)) : ''}
    <div id="web-search"></div>`;
  view.style.setProperty('--hue', '#2d2a3a');
  clearTimeout(renderSearch.t);
  renderSearch.t = setTimeout(() => webSearch(q), 450);
}
/* "From the catalog" (Apple's catalog) plus a YouTube search for things the catalog doesn't have (remixes, edits, new uploads). */
const WEB_SHOWN = 8;
let web = { q: '', cat: null, catErr: '', yt: null, ytErr: '', more: false };
const webRow = (i, act) => `<div class="wrow"><div class="art" style="background-image:url('${esc(i.thumbnail)}')"></div><div style="min-width:0"><div class="tt">${esc(i.title)}</div><div class="ta">${esc(i.artist)}${i.album ? ' · ' + esc(i.album) : ''}${act === 'add-yt' && i.duration ? ' · ' + fmt(i.duration) : ''}</div></div><button class="add-btn ${i.in_library ? 'done' : ''}" data-act="${act}" data-key="${esc(i.id)}" ${i.in_library ? 'disabled' : ''}>${i.in_library ? 'In library' : 'Add'}</button></div>`;
function drawWeb() {
  const box = $('#web-search'); if (!box || S.q.trim() !== web.q) return;
  const cat = web.cat, yt = web.yt;
  let html = `<div class="sect-title">From the catalog</div>`;
  if (cat === null && !web.catErr) html += `<div class="note">Searching…</div>`;
  else if (web.catErr) html += `<div class="note">${esc(web.catErr)} <button class="chip" data-act="web-retry">Retry</button></div>`;
  else {
    const list = web.more ? cat : cat.slice(0, WEB_SHOWN);
    html += `<div class="web">${list.map(i => webRow(i, 'add-catalog')).join('') || '<div class="note">No catalog results.</div>'}</div>`;
    if (!web.more && cat.length > WEB_SHOWN) html += `<div class="web-more"><button class="chip" data-act="web-more">Show ${cat.length - WEB_SHOWN} more</button></div>`;
  }
  if (yt === 'loading') html += `<div class="sect-title">From YouTube</div><div class="note">Searching YouTube…</div>`;
  else if (Array.isArray(yt)) html += `<div class="sect-title">From YouTube</div><div class="web">${yt.map(i => webRow(i, 'add-yt')).join('') || '<div class="note">No YouTube results.</div>'}</div>`;
  else if (web.ytErr) html += `<div class="sect-title">From YouTube</div><div class="note">${esc(web.ytErr)} <button class="chip" data-act="web-yt">Retry</button></div>`;
  else if (cat !== null || web.catErr) html += `<div class="web-more">${cat && cat.length < 3 ? '<div class="note" style="margin-bottom:8px">Not many catalog results. Remixes, edits and new uploads are usually on YouTube.</div>' : ''}<button class="chip" data-act="web-yt">Search YouTube</button></div>`;
  box.innerHTML = html;
}
async function webYouTube() {
  const q = web.q; if (web.yt === 'loading') return;
  web.yt = 'loading'; web.ytErr = ''; drawWeb();
  let results;
  try { results = (await api(`/api/search?source=youtube&q=${enc(q)}`)).results; }
  catch (e) { if (web.q === q) { web.yt = null; web.ytErr = e.message; drawWeb(); } return; }
  if (web.q !== q) return;
  results.forEach(i => { S.webItems[i.id] = i; });
  web.yt = results; drawWeb();
}
async function webSearch(q) {
  if (!$('#web-search') || S.q.trim() !== q) return;
  web = { q, cat: null, catErr: '', yt: null, ytErr: '', more: false };
  drawWeb();
  let results;
  try { results = (await api(`/api/search?q=${enc(q)}`)).results; }
  catch (e) { if (web.q === q) { web.catErr = e.message; drawWeb(); } return; }
  if (web.q !== q) return;
  S.webItems = { ...S.webItems, ...Object.fromEntries(results.map(i => [i.id, i])) };
  web.cat = results; drawWeb();
  if (results.length < 3 && !web.catErr) webYouTube();
}

function renderAdd() {
  $('#view').innerHTML = `<h1 class="big">Add music</h1>
    <div class="note" style="margin-top:6px;max-width:560px">Paste a YouTube or Spotify link for a song or a whole playlist, or search the catalog from the search bar and press Add.</div>
    <div class="drop"><input class="field" id="add-url" placeholder="https://…" aria-label="Link" style="flex:1;min-width:240px">
      <button class="pill-btn" data-act="add-url" data-mode="track">Add song</button><button class="pill-btn ghost" data-act="add-url" data-mode="playlist">Add playlist</button></div>
    <div id="deps-banner"></div>
    <div class="jobs" id="jobs"></div><div id="web-recs"></div>`;
  $('#view').style.setProperty('--hue', '#3a2418');
  renderJobs(); loadWebRecs(); refreshDeps();
}

/* yt-dlp / ffmpeg: show whether they are installed, and install them with winget if not. */
let depsTimer = 0;
async function refreshDeps() {
  let d;
  try { d = await api('/api/deps'); } catch { return; }
  const missing = [!d.yt_dlp && 'yt-dlp', !d.ffmpeg && 'ffmpeg'].filter(Boolean);
  const st = $('#deps-status'), btn = $('#deps-btn'), banner = $('#deps-banner');
  const text = d.installing ? 'Installing… this can take a minute.'
    : missing.length ? `Missing: ${missing.join(' and ')}.${d.error ? ' ' + d.error : ''}${d.winget ? '' : ' winget was not found, so install them by hand.'}`
    : 'yt-dlp and ffmpeg are installed.';
  if (st) st.textContent = text;
  if (btn) { btn.hidden = !missing.length || !d.winget; btn.disabled = d.installing; btn.textContent = d.installing ? 'Installing…' : 'Install missing tools'; }
  if (banner) {
    banner.innerHTML = missing.length ? `<div class="start"><h2>Vinyl needs ${missing.join(' and ')} to add music</h2><p>${esc(text)}</p>${d.winget && !d.installing ? '<div class="drop"><button class="pill-btn" data-act="install-deps">Install now</button></div>' : ''}</div>` : '';
  }
  clearTimeout(depsTimer);
  if (d.installing) depsTimer = setTimeout(refreshDeps, 2000);
  else if (S.depsWas && !missing.length) toast('Download tools are ready');
  S.depsWas = d.installing;
}
function renderJobs() {
  const box = $('#jobs'); if (!box) return;
  box.innerHTML = S.jobs.slice().reverse().map(j => `<div class="job ${j.status === 'error' ? 'err' : ''}"><div>${esc(j.label)} — ${j.status === 'done' ? esc(j.summary || 'added') : j.status === 'error' ? esc(j.error || 'failed') : esc(j.detail || j.percent || 'downloading…')}</div>${j.status === 'downloading' ? `<div class="pb"><i style="width:${Math.round((j.progress ?? j.fraction ?? 0) * 100)}%"></i></div>` : ''}</div>`).join('');
}

/* Reload the library while a big import is still running, so songs show up one by one. */
let lastLiveRefresh = 0;
function refreshWhileImporting() {
  if (Date.now() - lastLiveRefresh < 2500) return;
  lastLiveRefresh = Date.now();
  loadAll().then(() => {
    renderLibrary();
    if (['home', 'library', 'liked', 'playlist', 'playlists', 'artist'].includes(route.name)) renderRoute(true);
  }).catch(() => { });
}
async function startJob(label, body, { quiet = false } = {}) {
  const job = { label, status: 'downloading', fraction: 0 };
  S.jobs.push(job); renderJobs();
  try {
    const { job_id } = await api('/api/download', { method: 'POST', body });
    for (;;) {
      await new Promise(r => setTimeout(r, 1000));
      const j = await api(`/api/download/${job_id}`);
      const nAdded = j.added?.length || 0, nSkipped = j.skipped?.length || 0, nFailed = j.failed?.length || 0;
      const done = nAdded + nSkipped + nFailed, total = j.total || 0;
      Object.assign(job, { status: j.status, percent: j.percent, fraction: j.fraction, error: j.error, fileId: j.id });
      if (total) { // a playlist import: the bar moves with every finished song
        job.progress = Math.min(1, (done + (j.status === 'downloading' ? (j.fraction || 0) : 0)) / total);
        job.detail = `${done} of ${total}${j.current_title && j.status === 'downloading' ? ' · ' + j.current_title : ''}`;
        job.summary = `${nAdded} added${nSkipped ? `, ${nSkipped} already in your library` : ''}${nFailed ? `, ${nFailed} failed` : ''}`;
        if (nAdded !== (job.lastAdded || 0)) { job.lastAdded = nAdded; refreshWhileImporting(); }
      }
      renderJobs();
      if (j.status !== 'downloading') break;
    }
  } catch (e) { job.status = 'error'; job.error = e.message; }
  renderJobs();
  if (job.status === 'done') { if (!quiet) toast(`Added ${label}`); await loadAll(); renderLibrary(); if (['home', 'library', 'liked'].includes(route.name)) renderRoute(true); }
  else toast(`Couldn't add ${label}`);
  return job;
}

function renderQueue() {
  const cur = P.cur, ctxNext = P.order.slice(P.pos + 1, P.pos + 41);
  $('#view').innerHTML = `<h1 class="big">Queue</h1>
    ${cur ? `<div class="sect-title">Now playing</div>${trackRows([cur.id], { album: false })}` : '<div class="empty"><b>Nothing is playing</b></div>'}
    ${P.userQ.length ? `<div class="sect-title" style="display:flex;justify-content:space-between;align-items:center">Next in queue <button class="chip" data-act="clear-queue">Clear queue</button></div><div class="note" style="margin:-4px 0 6px">Drag a song to reorder, or press × to remove it.</div>${trackRows(P.userQ, { album: false, sort: 'queue-user', queueSec: 'user' })}` : ''}
    ${ctxNext.length ? `<div class="sect-title">Next from ${esc(P.label || 'your library')}</div>${P.userQ.length ? '' : '<div class="note" style="margin:-4px 0 6px">Drag a song to reorder, or press × to remove it.</div>'}${trackRows(ctxNext, { album: false, sort: 'queue-ctx', queueSec: 'ctx' })}` : ''}`;
  $('#view').style.setProperty('--hue', '#3a2a22');
}

function queueChanged() { renderRoute(true); renderNP(); saveStateSoon(); }
function queueRemove(sec, i) {
  if (sec === 'user') P.userQ.splice(i, 1);
  else {
    const at = P.pos + 1 + i, id = P.order[at];
    P.order.splice(at, 1);
    const j = P.ctx.indexOf(id); if (j >= 0 && !P.shuffle) P.ctx.splice(j, 1);
  }
  queueChanged();
}
function queueJump(sec, i) {
  if (sec === 'user') { const [id] = P.userQ.splice(i, 1); if (id) loadTrack(id, true); }
  else { P.pos = P.pos + 1 + i; loadTrack(P.order[P.pos], true); }
  if (route.name === 'queue') renderRoute(true);
}
const moveItem = (arr, from, to) => { const [x] = arr.splice(from, 1); arr.splice(to, 0, x); };

/* ---- drag to reorder: queue sections, playlist songs and the playlists in the sidebar ---- */
const SORT_ITEMS = ':scope > .row, :scope > .lib-item';
async function onSort(kind, from, to) {
  if (kind === 'queue-user') { moveItem(P.userQ, from, to); queueChanged(); }
  else if (kind === 'queue-ctx') {
    const base = P.pos + 1; moveItem(P.order, base + from, base + to);
    if (!P.shuffle) P.ctx = P.order.slice();
    queueChanged();
  } else if (kind.startsWith('playlist:')) {
    const pid = kind.slice(9), pl = S.playlists.find(p => p.id === pid); if (!pl) return;
    const ids = (pl.track_ids || []).filter(id => S.by.has(id)); moveItem(ids, from, to);
    pl.track_ids = ids; S.ver++; renderRoute(true); renderLibrary();
    try { await api(`/api/playlists/${pid}/order`, { method: 'PUT', body: { track_ids: ids } }); }
    catch { toast("Couldn't save the new order"); await refreshPlaylists(); renderRoute(true); }
  } else if (kind === 'playlists') {
    const ids = S.playlists.map(p => p.id); moveItem(ids, from, to);
    S.playlists = pinSort(ids.map(id => S.playlists.find(p => p.id === id))); S.ver++; renderLibrary();
    const ordered = S.playlists.map(p => p.id);
    try { await api('/api/playlists/order', { method: 'PUT', body: { ids: ordered } }); }
    catch { toast("Couldn't save the new order"); await refreshPlaylists(); }
  }
}
/* ---- carrying songs: drop them on a playlist in the sidebar, on Liked Songs, on Queue or on "+" ---- */
const DROP_TARGETS = '.lib-item[data-pl], .nav-link[data-nav="liked"], .nav-link[data-nav="queue"], [data-act="new-playlist"]';
const dropTargetAt = (x, y) => document.elementFromPoint(x, y)?.closest(DROP_TARGETS) || null;
async function likeMany(ids) {
  const todo = ids.filter(id => !isLiked(id));
  if (!todo.length) { toast('Already in Liked Songs'); return; }
  todo.forEach(id => S.likes.set(id, Date.now() / 1000));
  S.ver++; syncHearts(); renderLibrary();
  try {
    await Promise.all(todo.map(id => api(`/api/likes/${enc(id)}`, { method: 'PUT' })));
    toast(todo.length > 1 ? `${todo.length} songs added to Liked Songs` : 'Added to Liked Songs');
  } catch { todo.forEach(id => S.likes.delete(id)); syncHearts(); toast("Couldn't update Liked Songs"); }
  if (route.name === 'liked') renderRoute(true);
}
async function dropSongs(target, ids) {
  if (target.matches('.lib-item[data-pl]')) await addToPlaylist(target.dataset.pl, ids);
  else if (target.matches('[data-nav="liked"]')) await likeMany(ids);
  else if (target.matches('[data-nav="queue"]')) queueEnd(ids);
  else if (target.matches('[data-act="new-playlist"]')) { const pl = await newPlaylist(); if (pl) await addToPlaylist(pl.id, ids); }
}

/* One drag system for the lists: rows in a sortable list reorder (line shows where), and any song row
   (selected rows travel together) can be carried to a drop target in the sidebar. */
(function enableDrag() {
  let drag = null, timer = 0, lastX = 0, lastY = 0, hover = null;
  const edge = 56; // px from the edge of the scroll area where dragging starts to scroll it
  const clearHover = () => { hover?.classList.remove('drop-target'); hover = null; };
  const stop = () => {
    clearInterval(timer); clearHover(); document.body.classList.remove('dragging-rows');
    drag?.line?.remove(); drag?.ghost?.remove(); drag?.item.classList.remove('dragging');
  };
  document.addEventListener('mousedown', e => {
    if (e.button !== 0 || e.target.closest('button:not(.lib-item),a,input,select,textarea')) return;
    const item = e.target.closest('[data-sort] > .row, [data-sort] > .lib-item, .tl > .row');
    if (!item || item.classList.contains('web-row')) return;
    const kind = item.parentElement.dataset.sort || null;
    const carry = item.matches('.row') && !!item.dataset.id && !item.dataset.id.startsWith('pv:');
    drag = { item, box: item.parentElement, kind, carry, startX: e.clientX, startY: e.clientY, started: false };
    drag.from = kind ? [...drag.box.querySelectorAll(SORT_ITEMS)].indexOf(item) : -1;
  });
  document.addEventListener('mousemove', e => {
    if (!drag) return;
    lastX = e.clientX; lastY = e.clientY;
    if (!drag.started) {
      if (Math.hypot(e.clientX - drag.startX, e.clientY - drag.startY) < 6) return;
      drag.started = true; drag.item.classList.add('dragging'); document.body.classList.add('dragging-rows');
      if (drag.kind) {
        drag.line = document.createElement('div'); drag.line.className = 'drop-line'; document.body.append(drag.line);
        drag.scroller = drag.box.closest('.view, .lib-list');
      }
      if (drag.carry) { // which songs travel: the selected rows, if this row is one of them
        const sel = [...document.querySelectorAll('.row.sel')].filter(r => r.dataset.id && !r.dataset.id.startsWith('pv:'));
        drag.ids = sel.includes(drag.item) && sel.length > 1 ? [...new Set(sel.map(r => r.dataset.id))] : [drag.item.dataset.id];
        drag.ghost = document.createElement('div'); drag.ghost.className = 'drag-ghost';
        drag.ghost.textContent = drag.ids.length > 1 ? `${drag.ids.length} songs` : (trackOf(drag.ids[0])?.title || 'Song');
        document.body.append(drag.ghost);
      }
      timer = setInterval(tick, 30);
    }
    move();
  });
  function tick() { // keep scrolling while the pointer rests near an edge
    const r = drag?.scroller?.getBoundingClientRect();
    if (r) { if (lastY < r.top + edge) drag.scroller.scrollTop -= 14; else if (lastY > r.bottom - edge) drag.scroller.scrollTop += 14; }
    if (drag) move();
  }
  function move() {
    if (drag.ghost) Object.assign(drag.ghost.style, { left: lastX + 14 + 'px', top: lastY + 10 + 'px' });
    const target = drag.carry ? dropTargetAt(lastX, lastY) : null;
    if (target !== hover) { clearHover(); if (target) { target.classList.add('drop-target'); hover = target; } }
    drag.target = target;
    if (drag.line) { drag.line.style.display = target ? 'none' : ''; if (!target) place(lastY); }
  }
  function place(y) {
    const items = [...drag.box.querySelectorAll(SORT_ITEMS)];
    let to = items.length;
    for (let i = 0; i < items.length; i++) { const r = items[i].getBoundingClientRect(); if (y < r.top + r.height / 2) { to = i; break; } }
    drag.to = to;
    const ref = items[Math.min(to, items.length - 1)].getBoundingClientRect(), box = drag.box.getBoundingClientRect();
    Object.assign(drag.line.style, { left: box.left + 'px', width: box.width + 'px', top: (to < items.length ? ref.top : ref.bottom) - 1 + 'px' });
  }
  document.addEventListener('mouseup', () => {
    if (!drag) return;
    const d = drag; stop(); drag = null;
    if (!d.started) return;
    const swallow = ev => { ev.stopPropagation(); ev.preventDefault(); };
    window.addEventListener('click', swallow, { capture: true, once: true }); // the drop must not also count as a click
    setTimeout(() => window.removeEventListener('click', swallow, true), 0);
    if (d.target && d.carry) { dropSongs(d.target, d.ids); return; }
    if (!d.kind) return;
    let to = d.to; if (to > d.from) to--;
    if (to !== d.from) onSort(d.kind, d.from, to);
  });
  document.addEventListener('keydown', e => { if (e.key === 'Escape' && drag?.started) { stop(); drag = null; } });
  addEventListener('blur', () => { if (drag) { stop(); drag = null; } });
})();

/* ---- adding songs from this computer ---- */
const AUDIO_RE = /\.(mp3|wav|m4a|flac|ogg|opus|aac)$/i;
function pickSongs(playlistId) {
  const input = document.createElement('input');
  input.type = 'file'; input.multiple = true; input.accept = '.mp3,.wav,.m4a,.flac,.ogg,.opus,.aac,audio/*';
  input.onchange = () => uploadSongs([...input.files], { playlistId });
  input.click();
}
/* Files and whole folders from a drop. The entries must be taken before the first await. */
async function filesFromDrop(dt) {
  const out = [];
  const entries = [...(dt.items || [])].map(i => i.webkitGetAsEntry?.()).filter(Boolean);
  const files = [...(dt.files || [])];
  const walk = async entry => {
    if (entry.isFile) await new Promise(res => entry.file(f => { out.push(f); res(); }, res));
    else if (entry.isDirectory) {
      const reader = entry.createReader();
      for (;;) {
        const batch = await new Promise(res => reader.readEntries(res, () => res([])));
        if (!batch.length) break;
        for (const e of batch) await walk(e);
      }
    }
  };
  if (entries.length) for (const e of entries) await walk(e); else out.push(...files);
  return out;
}
let uploadChain = Promise.resolve();
const uploadSongs = (files, opts) => (uploadChain = uploadChain.then(() => doUpload(files, opts)).catch(() => { }));
async function refreshAfterUpload() {
  await loadAll(); renderLibrary();
  if (['home', 'library', 'albums', 'album', 'liked', 'playlist'].includes(route.name)) renderRoute(true);
}
async function doUpload(files, { playlistId = null } = {}) {
  files = files.filter(f => AUDIO_RE.test(f.name));
  if (!files.length) { toast('Drop audio files: MP3, M4A, FLAC, WAV, OGG, OPUS or AAC'); return; }
  let tray = $('#uptray');
  if (!tray) { tray = document.createElement('div'); tray.id = 'uptray'; tray.className = 'uptray'; document.body.append(tray); }
  const show = (i, name) => { tray.innerHTML = `<div class="t">Adding songs · ${i + 1} of ${files.length}</div><div class="s">${esc(name)}</div><div class="pb"><i style="width:${Math.round(i / files.length * 100)}%"></i></div>`; };
  let added = 0, dupes = 0, failed = 0; const ids = [];
  for (let i = 0; i < files.length; i++) {
    show(i, files[i].name);
    try {
      const r = await fetch('/api/upload?name=' + enc(files[i].name), { method: 'POST', body: files[i] });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(j.error || r.statusText);
      ids.push(j.id); if (j.duplicate) dupes++; else added++;
    } catch { failed++; }
    if ((i + 1) % 4 === 0 && i + 1 < files.length) await refreshAfterUpload(); // songs appear as they land
  }
  tray.remove();
  await refreshAfterUpload();
  if (playlistId && ids.length) await addToPlaylist(playlistId, ids);
  toast([added ? `Added ${plural(added, 'song')}` : '', dupes ? `${dupes} already in your library` : '', failed ? `${failed} failed` : ''].filter(Boolean).join(' · ') || 'Nothing added');
}
/* Drop files or folders anywhere in the window. Dropping on a playlist in the sidebar (or while looking
   at a playlist) puts the songs in that playlist too. */
(function enableFileDrop() {
  let depth = 0;
  const hasFiles = e => [...(e.dataTransfer?.types || [])].includes('Files');
  const zone = $('#dropzone');
  const playlistUnder = e => document.elementFromPoint(e.clientX, e.clientY)?.closest('.lib-item[data-pl]');
  const reset = () => { depth = 0; zone.hidden = true; $$('.lib-item.drop-target').forEach(x => x.classList.remove('drop-target')); };
  addEventListener('dragenter', e => {
    if (!hasFiles(e)) return;
    e.preventDefault(); depth++;
    const pl = route.name === 'playlist' ? S.playlists.find(p => p.id === route.arg) : null;
    $('b', zone).textContent = pl ? `Drop songs to add them to "${pl.name}"` : 'Drop songs or folders to add them to your library';
    zone.hidden = false;
  });
  addEventListener('dragover', e => {
    if (!hasFiles(e)) return;
    e.preventDefault(); e.dataTransfer.dropEffect = 'copy';
    const t = playlistUnder(e);
    $$('.lib-item.drop-target').forEach(x => { if (x !== t) x.classList.remove('drop-target'); });
    t?.classList.add('drop-target');
  });
  addEventListener('dragleave', e => { if (hasFiles(e) && --depth <= 0) reset(); });
  addEventListener('drop', e => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    const playlistId = playlistUnder(e)?.dataset.pl || (route.name === 'playlist' ? route.arg : null);
    const taking = filesFromDrop(e.dataTransfer); // called now: the drop's entries stop working after this event
    reset();
    taking.then(files => uploadSongs(files, { playlistId }));
  });
})();

/* ------------------------------------------------------- discover / genres */

let genreCache = null;
async function genres() { if (!genreCache) genreCache = (await dz('genres')).genres; return genreCache; }
const genreTile = (g, pick) => `<div class="gtile ${S.picked.has(String(g.id)) ? 'picked' : ''}" style="background-image:url('${esc(g.picture)}')" ${pick ? `data-act="pick-genre" data-id="${g.id}"` : `data-href="#/genre/${g.id}"`} tabindex="0" role="button" aria-label="${esc(g.name)}"><a class="gp" href="#/genre/${g.id}">${pick ? 'Preview' : 'Open'}</a><div class="gn">${esc(g.name)}</div></div>`;

function renderStart() {
  $('#view').innerHTML = `<div class="kicker">Welcome to Vinyl</div><h1 class="big">Start your crate</h1>
    <div class="start"><h2>Pick what you are into</h2>
      <p>Choose a few genres and Vinyl downloads the biggest songs from each, so there is something to play, then learns from what you play and like. Hit PREVIEW on a tile to listen first.</p>
      <div class="browse" id="genre-pick"><div class="note">Loading genres…</div></div>
      <div class="drop"><button class="pill-btn" data-act="build-library">Build my library</button><span class="note" id="pick-count">${S.picked.size} picked</span><a class="pill-btn ghost" href="#/add">Paste a link instead</a></div>
      <div class="jobs" id="jobs"></div>
    </div>`;
  $('#view').style.setProperty('--hue', '#2a1608');
  genres().then(gs => { const box = $('#genre-pick'); if (box) box.innerHTML = gs.map(g => genreTile(g, true)).join(''); renderJobs(); })
    .catch(e => { const box = $('#genre-pick'); if (box) box.innerHTML = `<div class="note">${esc(e.message)}</div>`; });
}

async function buildLibrary() {
  if (!S.picked.size) { toast('Pick at least one genre'); return; }
  toast('Finding songs…');
  const lists = [];
  for (const id of S.picked) { try { lists.push(((await dz(`genre?id=${id}`)).tracks || []).filter(t => !owned(t))); } catch { /* skip */ } }
  const items = stash(roundRobin(lists, Math.min(24, S.picked.size * 4)));
  if (!items.length) { toast("Couldn't reach the catalog"); return; }
  toast(`Adding ${items.length} songs…`);
  S.picked = new Set();
  for (const it of items) await startJob(`${it.title} — ${it.artist}`, { mode: 'catalog', catalog: it.catalog }, { quiet: true });
  toast('Your crate is ready');
  renderRoute(true);
}

function renderDiscover() {
  const T = taste(), seeds = T.top.slice(0, 2).map(k => T.names.get(k));
  $('#view').innerHTML = `<div class="kicker">Explore</div><h1 class="big">Discover</h1>
    <div class="statline">Hear previews, then add what you like</div>
    <div id="web-recs"></div>
    ${seeds.map((_, i) => `<div id="dz-like-${i}"></div>`).join('')}
    <section class="shelf"><div class="shelf-head"><div><h2>Genres</h2><div class="sub">Open one for its chart and top artists</div></div></div><div class="browse" id="genre-all"><div class="note">Loading…</div></div></section>`;
  $('#view').style.setProperty('--hue', '#2a1608');
  if (seeds.length) loadWebRecs();
  seeds.forEach((name, i) => similarFor(name, 10, 1).then(res => {
    const box = $(`#dz-like-${i}`); if (!box || route.name !== 'discover') return;
    const fresh = (res?.related || []).filter(a => !T.byArtist.has(a.name.toLowerCase()));
    if (fresh.length) box.innerHTML = shelf(`Artists like ${name}`, 'Not in your library yet', fresh.map(dzArtistCard));
  }));
  genres().then(gs => { const box = $('#genre-all'); if (box && route.name === 'discover') box.innerHTML = gs.map(g => genreTile(g, false)).join(''); })
    .catch(e => { const box = $('#genre-all'); if (box) box.innerHTML = `<div class="note">${esc(e.message)}</div>`; });
}

async function renderGenre(id) {
  const view = $('#view'); view.innerHTML = '<div class="note">Loading…</div>';
  let res, name = 'Genre';
  try { res = await dz(`genre?id=${enc(id)}`); name = (await genres()).find(g => String(g.id) === String(id))?.name || name; }
  catch (e) { view.innerHTML = `<div class="empty"><b>Offline</b>${esc(e.message)}</div>`; return; }
  if (route.name !== 'genre') return;
  const items = stash((res.tracks || []).filter(t => !owned(t)));
  S.view = { items };
  view.innerHTML = `<div class="kicker">Genre</div><h1 class="big">${esc(name)}</h1><div class="statline"><b>${items.length}</b> chart songs you don't have yet</div>
    <div class="actions" style="margin-top:18px"><button class="pill-btn" data-act="add-top">Add the top 10</button></div>
    ${items.length ? previewRows(items) : '<div class="empty"><b>You have the whole chart</b></div>'}
    ${res.artists?.length ? shelf('Popular artists', '', res.artists.map(dzArtistCard)) : ''}`;
  view.style.setProperty('--hue', '#2a1608');
  const first = $('.row', view); // the first play button should start the previews as one queue
  if (first) first.dataset.label = name;
}

async function renderDz(id) {
  const view = $('#view'); view.innerHTML = '<div class="note">Loading…</div>';
  let res;
  try { res = await dz(`artist?id=${enc(id)}`); } catch (e) { view.innerHTML = `<div class="empty"><b>Offline</b>${esc(e.message)}</div>`; return; }
  if (route.name !== 'dz') return;
  const a = res.artist, T = taste(), items = stash((res.tracks || []).filter(t => !owned(t)));
  S.view = { items };
  const have = T.byArtist.has(String(a.name).toLowerCase());
  view.innerHTML = `<div class="kicker">Artist${have ? ' · in your library' : ' · new to you'}</div>
    <div class="hero"><div class="art round" style="background-image:url('${esc(a.picture)}')"></div><div><h1 class="big">${esc(a.name)}</h1><div class="meta"><b>${a.fans ? Math.round(a.fans / 1000).toLocaleString() + 'k fans' : ''}</b></div></div></div>
    <div class="actions">${have ? `<a class="pill-btn ghost" href="#/artist/${enc(a.name)}">Your songs</a>` : ''}<button class="pill-btn" data-act="add-top">Add top songs</button></div>
    ${items.length ? previewRows(items) : '<div class="empty"><b>You have all their top songs</b></div>'}
    ${(res.related || []).length ? shelf('Similar artists', '', res.related.map(dzArtistCard)) : ''}`;
  view.style.setProperty('--hue', '#1f1410');
}

/* ----------------------------------------------------------------- library */

function renderLibrary() {
  const el = $('#lib-list'); if (!el) return;
  el.innerHTML = S.playlists.map(pl => `<button class="lib-item ${route.name === 'playlist' && route.arg === pl.id ? 'on' : ''}" data-href="#/playlist/${pl.id}" data-pl="${esc(pl.id)}">${playlistArt(pl)}<div class="txt"><div class="t">${esc(pl.name)}</div></div>${pl.pinned ? `<span class="pin" title="Pinned">${ICON.pin}</span>` : ''}</button>`).join('')
    || '<p class="side-empty">No playlists yet</p>';
}

/* ------------------------------------------------------------ menus + modal */

const menu = $('#menu');
function closeMenu() { menu.hidden = true; menu.innerHTML = ''; }
function openMenu(x, y, items) {
  menu.innerHTML = items.map((it, i) => it.sep ? '<div class="msep"></div>' : it.head ? `<div class="mhead">${esc(it.head)}</div>` : `<button class="mi ${it.danger ? 'danger' : ''}" data-mi="${i}">${it.icon || ''}${esc(it.label)}</button>`).join('');
  menu.hidden = false;
  const w = menu.offsetWidth, h = menu.offsetHeight;
  menu.style.left = Math.max(8, Math.min(x, innerWidth - w - 8)) + 'px';
  menu.style.top = Math.max(8, Math.min(y, innerHeight - h - 8)) + 'px';
  menu._items = items;
  menu.querySelector('.mi')?.focus();
}
menu.addEventListener('click', e => {
  const b = e.target.closest('[data-mi]'); if (!b) return;
  const it = menu._items[+b.dataset.mi]; const keep = it.keep;
  if (!keep) closeMenu();
  it.run?.();
});
document.addEventListener('mousedown', e => { if (!menu.hidden && !menu.contains(e.target)) closeMenu(); });
document.addEventListener('keydown', e => { if (e.key === 'Escape') { closeMenu(); closeModal(); if (S.np && menu.hidden && $('#modal').hidden) { S.np = false; applyNPState(); } } });
$('#view').addEventListener('scroll', () => closeMenu());

function trackMenu(id, x, y, fromPl) {
  const t = trackOf(id); if (!t) return;
  if (t.isPv) {
    openMenu(x, y, [{ label: 'Add to library', icon: ICON.plus, run: () => addPv(t) }, { label: 'Play next', icon: ICON.play, run: () => queueNext([id]) }, { label: 'Add to queue', icon: ICON.queue, run: () => queueEnd([id]) }]);
    return;
  }
  const items = [
    { label: isLiked(id) ? 'Remove from Liked Songs' : 'Save to Liked Songs', icon: ICON.heart, run: () => toggleLike(id) },
    { label: 'Add to playlist', icon: ICON.plus, keep: true, run: () => playlistPicker([id], x, y) },
    { sep: 1 },
    { label: 'Play next', icon: ICON.play, run: () => queueNext([id]) },
    { label: 'Add to queue', icon: ICON.queue, run: () => queueEnd([id]) },
    { label: 'Go to artist', icon: ICON.user, run: () => go(`#/artist/${enc(primaryArtist(t.artist))}`) },
  ];
  if (fromPl) items.push({ sep: 1 }, { label: 'Remove from this playlist', icon: ICON.trash, run: () => removeFromPlaylist(fromPl, id) });
  items.push({ sep: 1 }, { label: 'Delete from library…', icon: ICON.trash, danger: true, run: () => deleteTrack(t) });
  openMenu(x, y, items);
}
function playlistPicker(ids, x, y) {
  const items = [{ head: 'Add to playlist' }, { label: 'New playlist', icon: ICON.plus, run: async () => { const pl = await newPlaylist(); if (pl) await addToPlaylist(pl.id, ids); } }];
  for (const pl of S.playlists) items.push({ label: pl.name, run: () => addToPlaylist(pl.id, ids) });
  openMenu(x, y, items);
}
function pickPlaylistCover(pid) {
  const input = document.createElement('input');
  input.type = 'file'; input.accept = 'image/*';
  input.onchange = () => { const f = input.files?.[0]; if (f) setPlaylistCover(pid, f); };
  input.click();
}
/* The picture is cropped to a square and shrunk here, then the server stores it as the playlist's cover. */
async function setPlaylistCover(pid, file) {
  try {
    const bmp = await createImageBitmap(file), side = Math.min(bmp.width, bmp.height), out = 600;
    const c = document.createElement('canvas'); c.width = c.height = out;
    c.getContext('2d').drawImage(bmp, (bmp.width - side) / 2, (bmp.height - side) / 2, side, side, 0, 0, out, out);
    const blob = await new Promise(res => c.toBlob(res, 'image/jpeg', 0.88));
    const r = await fetch(`/api/playlists/${pid}/cover`, { method: 'POST', body: blob, headers: { 'Content-Type': 'image/jpeg' } });
    if (!r.ok) throw new Error(r.statusText);
    await refreshPlaylists(); renderRoute(true); toast('Cover updated');
  } catch { toast("Couldn't use that picture"); }
}
async function removePlaylistCover(pid) {
  try { await api(`/api/playlists/${pid}/cover`, { method: 'DELETE' }); } catch { toast("Couldn't remove the cover"); return; }
  await refreshPlaylists(); renderRoute(true);
}
/* Play / Play next / Add to queue for an album, mix, artist, playlist or Liked Songs. */
function sourceMenu(key, x, y) {
  const ids = sourceIds(key); if (!ids.length) { toast('Nothing in there yet'); return; }
  openMenu(x, y, [
    { label: 'Play', icon: ICON.play, run: () => playSource(key) },
    { label: 'Play next', icon: ICON.play, run: () => queueNext(ids) },
    { label: 'Add to queue', icon: ICON.queue, run: () => queueEnd(ids) },
  ]);
}

async function togglePin(pid) {
  const pl = S.playlists.find(p => p.id === pid); if (!pl) return;
  try { await api(`/api/playlists/${pid}`, { method: 'PATCH', body: { pinned: !pl.pinned } }); }
  catch (e) { toast(e.message); return; }
  await refreshPlaylists(); if (route.name === 'playlists') renderRoute(true);
  toast(pl.pinned ? 'Unpinned' : 'Pinned to the top');
}
function playlistMenu(pid, x, y) {
  const pl = S.playlists.find(p => p.id === pid); if (!pl) return;
  openMenu(x, y, [
    { label: 'Play', icon: ICON.play, run: () => playSource('playlist:' + pid) },
    { label: 'Play next', icon: ICON.play, run: () => queueNext(sourceIds('playlist:' + pid)) },
    { label: 'Add to queue', icon: ICON.queue, run: () => queueEnd(sourceIds('playlist:' + pid)) },
    { sep: 1 },
    { label: pl.pinned ? 'Unpin' : 'Pin to top', icon: ICON.pin, run: () => togglePin(pid) },
    { label: 'Change cover…', icon: ICON.image, run: () => pickPlaylistCover(pid) },
    ...(pl.cover ? [{ label: 'Remove cover', icon: ICON.trash, run: () => removePlaylistCover(pid) }] : []),
    { label: 'Rename', icon: ICON.edit, run: () => renamePlaylist(pid) },
    { label: 'Delete playlist', icon: ICON.trash, danger: true, run: () => deletePlaylist(pid) },
  ]);
}

function modal({ title, text = '', input = null, ok = 'OK', cancel = 'Cancel', danger = false }) {
  return new Promise(res => {
    const w = $('#modal');
    w.innerHTML = `<form class="modal" role="dialog" aria-modal="true" aria-label="${esc(title)}"><h3>${esc(title)}</h3>${text ? `<p>${esc(text)}</p>` : ''}${input !== null ? `<input class="field" name="v" value="${esc(input)}" maxlength="120" autocomplete="off" aria-label="${esc(title)}">` : ''}<div class="btns"><button type="button" class="chip" data-x="cancel">${esc(cancel)}</button><button class="pill-btn ${danger ? 'danger' : ''}">${esc(ok)}</button></div></form>`;
    w.hidden = false;
    const f = $('form', w), inp = $('input', w);
    (inp || $('.pill-btn', w)).focus(); inp?.select();
    const done = v => { w.hidden = true; w.innerHTML = ''; modal.close = null; res(v); };
    modal.close = () => done(null);
    f.onsubmit = e => { e.preventDefault(); done(inp ? inp.value.trim() : true); };
    $('[data-x]', w).onclick = () => done(null);
    w.onmousedown = e => { if (e.target === w) done(null); };
  });
}
function closeModal() { modal.close?.(); }

/* Importing a playlist link: keep it as a playlist, or just add its songs to the library. */
function askPlaylistChoice() {
  return new Promise(res => {
    const w = $('#modal');
    w.innerHTML = `<div class="modal" role="dialog" aria-modal="true" aria-label="Import playlist"><h3>Import this playlist?</h3><p>Save it as a playlist in Vinyl, or only add its songs to your library.</p>
      <div class="btns stack"><button class="pill-btn" data-c="playlist">Save as a playlist</button><button class="pill-btn ghost" data-c="songs">Just add the songs</button><button class="chip" data-c="">Cancel</button></div></div>`;
    w.hidden = false;
    const done = v => { w.hidden = true; w.innerHTML = ''; modal.close = null; res(v); };
    modal.close = () => done(null);
    $$('[data-c]', w).forEach(b => { b.onclick = () => done(b.dataset.c || null); });
    w.onmousedown = e => { if (e.target === w) done(null); };
    $('[data-c="playlist"]', w).focus();
  });
}

/* One playlist per artist: pick which artists (2+ songs, no playlist of that name yet) to turn into playlists. */
function artistGroups() {
  const have = new Set(S.playlists.map(p => p.name.trim().toLowerCase()));
  const by = new Map();
  for (const t of S.tracks) {
    const name = primaryArtist(t.artist);
    if (/^(unknown|various)/i.test(name)) continue;
    const g = by.get(name.toLowerCase()) || { name, ids: [] };
    g.ids.push(t.id); by.set(name.toLowerCase(), g);
  }
  return [...by.values()].filter(g => g.ids.length >= 2 && !have.has(g.name.toLowerCase())).sort((a, b) => b.ids.length - a.ids.length || a.name.localeCompare(b.name));
}
function askArtistPlaylists(groups) {
  return new Promise(res => {
    const w = $('#modal');
    w.innerHTML = `<div class="modal" role="dialog" aria-modal="true" aria-label="Playlists from artists"><h3>Playlists from artists</h3><p>Make a playlist for each artist you pick. Artists with a playlist already, or fewer than two songs, aren't listed.</p>
      <div class="pick-list">${groups.map((g, i) => `<label class="pick"><input type="checkbox" value="${i}" checked><span class="pk-name">${esc(g.name)}</span><span class="pk-n">${plural(g.ids.length, 'song')}</span></label>`).join('')}</div>
      <div class="btns"><button class="chip" data-c="">Cancel</button><button class="pill-btn" data-ok></button></div></div>`;
    w.hidden = false;
    const okBtn = $('[data-ok]', w), boxes = $$('input[type=checkbox]', w);
    const picked = () => boxes.filter(b => b.checked).map(b => groups[+b.value]);
    const sync = () => { const n = picked().length; okBtn.textContent = n ? `Create ${plural(n, 'playlist')}` : 'Create'; okBtn.disabled = !n; };
    boxes.forEach(b => { b.onchange = sync; }); sync();
    const done = v => { w.hidden = true; w.innerHTML = ''; modal.close = null; res(v); };
    modal.close = () => done(null);
    okBtn.onclick = () => done(picked());
    $('[data-c]', w).onclick = () => done(null);
    w.onmousedown = e => { if (e.target === w) done(null); };
    okBtn.focus();
  });
}
async function playlistsFromArtists() {
  const groups = artistGroups();
  if (!groups.length) { toast('No artists with two or more songs are left without a playlist'); return; }
  const chosen = await askArtistPlaylists(groups);
  if (!chosen?.length) return;
  let made = 0;
  try {
    for (const g of chosen) {
      const pl = await api('/api/playlists', { method: 'POST', body: { name: g.name } });
      await api(`/api/playlists/${pl.id}/tracks`, { method: 'POST', body: { track_ids: g.ids } });
      made++;
    }
  } catch (e) { toast(e.message); }
  await refreshPlaylists();
  if (route.name === 'playlists') renderRoute(true);
  if (made) toast(`Created ${plural(made, 'playlist')}`);
}

async function newPlaylist() {
  const name = await modal({ title: 'Create playlist', input: `My Playlist #${S.playlists.length + 1}`, ok: 'Create' });
  if (!name) return null;
  const pl = await api('/api/playlists', { method: 'POST', body: { name } });
  await refreshPlaylists(); toast(`Created ${name}`);
  return pl;
}
async function renamePlaylist(pid) {
  const pl = S.playlists.find(p => p.id === pid); if (!pl) return;
  const name = await modal({ title: 'Rename playlist', input: pl.name, ok: 'Save' });
  if (!name) return;
  await api(`/api/playlists/${pid}`, { method: 'PATCH', body: { name } });
  await refreshPlaylists(); renderRoute(true);
}
async function deletePlaylist(pid) {
  const pl = S.playlists.find(p => p.id === pid); if (!pl) return;
  if (!await modal({ title: 'Delete playlist?', text: `"${pl.name}" will be removed. Your songs stay in the library.`, ok: 'Delete', danger: true })) return;
  await api(`/api/playlists/${pid}`, { method: 'DELETE' });
  await refreshPlaylists(); if (route.name === 'playlist') go('#/home'); toast('Playlist deleted');
}
async function addToPlaylist(pid, ids) {
  try { await api(`/api/playlists/${pid}/tracks`, { method: 'POST', body: { track_ids: ids } }); }
  catch (e) { toast(e.message); return; }
  const pl = S.playlists.find(p => p.id === pid);
  await refreshPlaylists(); toast(`Added to ${pl?.name || 'playlist'}`);
  if (route.name === 'playlist' && route.arg === pid) renderRoute(true);
}
async function removeFromPlaylist(pid, id) {
  await api(`/api/playlists/${pid}/tracks/${enc(id)}`, { method: 'DELETE' });
  await refreshPlaylists(); renderRoute(true); toast('Removed from playlist');
}
async function deleteTrack(t) {
  if (!await modal({ title: 'Delete from library?', text: `"${t.title}" will be deleted from disk. This cannot be undone.`, ok: 'Delete', danger: true })) return;
  await api(`/api/tracks/${enc(t.id)}`, { method: 'DELETE' });
  const wasCur = P.cur?.id === t.id;
  if (wasCur) { audio.pause(); P.pos = Math.max(-1, P.order.indexOf(t.id) - 1); }
  P.ctx = P.ctx.filter(x => x !== t.id); P.order = P.order.filter(x => x !== t.id); P.userQ = P.userQ.filter(x => x !== t.id);
  await loadAll();
  if (wasCur) { P.cur = null; if (P.order.length || P.userQ.length) next(false); else { audio.removeAttribute('src'); renderBarTrack(); renderNP(); } } renderLibrary(); renderRoute(true); toast('Deleted');
}

/* ------------------------------------------------------------------ router */

let route = { name: 'home', arg: '' };
const ROUTES = ['home', 'albums', 'album', 'settings', 'playlists', 'search', 'library', 'liked', 'playlist', 'artist', 'mix', 'add', 'queue', 'discover', 'genre', 'dz'];
function parseHash() {
  const [, name = 'home', ...rest] = (location.hash || '#/home').split('/');
  const arg = decodeURIComponent(rest.join('/'));
  return ROUTES.includes(name) ? { name, arg } : { name: 'home', arg: '' };
}
function go(h) {
  if (S.np) { S.np = false; applyNPState(); } // leaving Now Playing, even for the page that is already underneath
  if (location.hash === h) renderRoute(true); else location.hash = h;
}
window.addEventListener('hashchange', () => renderRoute(false));

function renderRoute(keepScroll) {
  route = parseHash();
  S.lists = {};
  const view = $('#view'), top = keepScroll ? view.scrollTop : 0;
  if (route.name !== 'search') { S.q = ''; $('#q').value = ''; }
  $$('.tab').forEach(a => a.classList.toggle('on', a.dataset.nav === route.name || (route.name === 'genre' || route.name === 'dz') && a.dataset.nav === 'discover'));
  shelfN = 0;
  if (!keepScroll && S.np) { S.np = false; applyNPState(); }
  switch (route.name) {
    case 'home': renderHome(); break;
    case 'search': renderSearch(); break;
    case 'library': renderLibrary_(); break;
    case 'liked': renderLiked(); break;
    case 'playlist': renderPlaylist(route.arg); break;
    case 'artist': renderArtist(route.arg); break;
    case 'mix': renderMix(route.arg); break;
    case 'add': renderAdd(); break;
    case 'queue': renderQueue(); break;
    case 'playlists': renderPlaylists(); break;
    case 'settings': renderSettings(); break;
    case 'albums': renderAlbums(); break;
    case 'album': renderAlbum(route.arg); break;
    case 'discover': renderDiscover(); break;
    case 'genre': renderGenre(route.arg); break;
    case 'dz': renderDz(route.arg); break;
  }
  view.scrollTop = top;
  $('#b-queue').classList.toggle('on', route.name === 'queue');
  renderLibrary(); markNow(); onScroll();
  document.title = 'Vinyl';
}
function onScroll() { }
$('#view').addEventListener('scroll', onScroll);
$('#q').addEventListener('input', e => {
  S.q = e.target.value;
  if (route.name !== 'search') { location.hash = '#/search'; return; }
  clearTimeout(onSearchInput.t); onSearchInput.t = setTimeout(renderSearch, 120);
});
const onSearchInput = {};
$('#q').addEventListener('focus', () => { if (route.name !== 'search') location.hash = '#/search'; });

/* -------------------------------------------------------------- interaction */

document.addEventListener('click', async e => {
  const t = e.target;
  if (t.closest('a[href^="#/"]')) { // plain links just navigate...
    // ...but if the page under Now Playing is already that page (Songs while on Songs), the hash
    // doesn't change and nothing would close the stage, so close it here.
    if (S.np) { S.np = false; applyNPState(); }
    return;
  }
  const nav = t.closest('[data-nav]'); if (nav && nav.tagName === 'BUTTON') { go(nav.dataset.nav); return; }
  const actEl = t.closest('[data-act]');
  if (actEl) {
    const a = actEl.dataset.act, d = actEl.dataset;
    e.stopPropagation();
    const r = actEl.getBoundingClientRect();
    switch (a) {
      case 'like': toggleLike(d.id); return;
      case 'more': trackMenu(d.id, r.left, r.bottom + 4, d.from); return;
      case 'play-one': {
        const tr = trackOf(d.id); if (!tr) return;
        playFrom(d.id);
        return;
      }
      case 'play-card': playSource(d.play); return;
      case 'shuffle-card': playSource(d.play, { shuffle: true }); return;
      case 'play-ids': playSource(d.key); return;
      case 'install-deps': { try { await api('/api/deps/install', { method: 'POST' }); } catch (e) { toast(e.message); return; } S.depsWas = true; refreshDeps(); return; }
      case 'check-update': { const i = await checkForUpdate({ manual: true }); const el = $('#upd-ver'); if (el && i) el.textContent = `Vinyl ${i.current}${i.available ? ` · ${i.latest} is available` : ' · up to date'}`; return; }
      case 'refresh-json': { actEl.disabled = true; try { const r = await api('/api/data/refresh', { method: 'POST' }); toast(`Refreshed ${r.files} files`); } catch (e) { toast(e.message); } actEl.disabled = false; return; }
      case 'reset-recs': wipe('recs'); return;
      case 'delete-playlists': wipe('playlists'); return;
      case 'delete-all': wipe('all'); return;
      case 'play-pv': playPv(d.key, actEl); return;
      case 'add-pv': { const pt = trackOf(d.id); if (pt) addPv(pt); return; }
      case 'play-previews': { const ids = (S.lists[d.list] || []); setContext(ids, 0, { label: d.label || 'Previews' }); return; }
      case 'add-top': { const items = (S.view?.items || []).filter(i => !owned(i)).slice(0, 10); toast(`Adding ${items.length} songs…`); autoAddNews(items, null); return; }
      case 'pick-genre': { const id = d.id; if (S.picked.has(id)) S.picked.delete(id); else S.picked.add(id); actEl.classList.toggle('picked', S.picked.has(id)); const c = $('#pick-count'); if (c) c.textContent = `${S.picked.size} picked`; return; }
      case 'build-library': buildLibrary(); return;
      case 'add-all-new': { const m = mixes().find(x => String(x.n) === d.n); if (m) { toast(`Adding ${mixNews(m).length} songs…`); autoAddNews(mixNews(m), null); } return; }
      case 'radio': { const tr = trackOf(d.id); if (tr) startRadio(tr); return; }
      case 'artist-playlists': playlistsFromArtists(); return;
      case 'new-playlist': { const pl = await newPlaylist(); if (pl) go('#/playlist/' + pl.id); return; }
      case 'add-songs': pickSongs(d.pl || null); return;
      case 'pl-cover': pickPlaylistCover(d.id); return;
      case 'source-menu': sourceMenu(d.play, r.left, r.bottom + 4); return;
      case 'playlist-menu': playlistMenu(d.id, r.left, r.bottom + 4); return;
      case 'goto-add': go('#/add'); return;
      case 'sort': S.sort = d.sort; lsSet('vinyl.sort', S.sort); renderRoute(true); return;
      case 'queue-remove': queueRemove(d.sec, +d.i); return;
      case 'clear-queue': P.userQ = []; renderRoute(true); renderNP(); saveStateSoon(); return;
      case 'refresh-lyrics': if (P.cur) loadLyrics(P.cur, true); return;
      case 'scroll-shelf': { const g = actEl.closest('.shelf').querySelector('.grid'); g.scrollBy({ left: +d.dir * Math.max(240, g.clientWidth * 0.85), behavior: 'smooth' }); return; }
      case 'toggle-shelf': { const g = actEl.closest('.shelf').querySelector('.grid'); const open = g.classList.toggle('open'); actEl.textContent = open ? 'Show less' : 'Show all'; actEl.setAttribute('aria-expanded', open); return; }
      case 'add-to-pl': await addToPlaylist(d.pid, [d.id]); actEl.textContent = 'Added'; actEl.disabled = true; actEl.classList.add('done'); return;
      case 'web-more': web.more = true; drawWeb(); return;
      case 'web-yt': webYouTube(); return;
      case 'web-retry': webSearch(S.q.trim()); return;
      case 'add-yt': {
        const item = S.webItems?.[d.key]; if (!item) return;
        actEl.disabled = true; actEl.textContent = 'Adding…';
        startJob(item.title, { url: item.url, mode: 'track', as_playlist: true }).then(j => { if (j.status === 'done') { actEl.textContent = 'In library'; actEl.classList.add('done'); } else { actEl.textContent = 'Add'; actEl.disabled = false; } });
        return;
      }
      case 'add-catalog': {
        const item = S.webItems?.[d.key]; if (!item) return;
        actEl.disabled = true; if (actEl.classList.contains('add-btn')) actEl.textContent = 'Adding…'; else actEl.innerHTML = ICON.check;
        startJob(`${item.title} — ${item.artist}`, { mode: 'catalog', catalog: item.catalog }).then(() => { if (actEl.classList.contains('add-btn')) { actEl.textContent = 'In library'; actEl.classList.add('done'); } });
        return;
      }
      case 'add-url': {
        const url = $('#add-url').value.trim(); if (!url) { toast('Paste a link first'); return; }
        const isList = d.mode === 'playlist' || /spotify\.com\/(playlist|album)\//i.test(url) || /[?&]list=|youtube\.com\/playlist/i.test(url);
        if (!isList) { $('#add-url').value = ''; startJob('Song', { url, mode: 'track', as_playlist: true }); return; }
        const choice = await askPlaylistChoice();
        if (!choice) return;
        $('#add-url').value = '';
        startJob(choice === 'playlist' ? 'Playlist' : 'Songs', { url, mode: 'playlist', as_playlist: choice === 'playlist' });
        return;
      }
    }
  }
  const lyr = t.closest('.lyr p[data-t]'); if (lyr) { audio.currentTime = +lyr.dataset.t; return; }
  const chip = t.closest('#lib-chips .chip'); if (chip) { S.libFilter = chip.dataset.filter; renderLibrary(); return; }
  const row = t.closest('.row');
  if (row) {
    const rows = [...row.parentElement.querySelectorAll(':scope > .row')];
    if (e.shiftKey && S.anchorRow && rows.includes(S.anchorRow)) { // a range
      const [lo, hi] = [rows.indexOf(S.anchorRow), rows.indexOf(row)].sort((x, y) => x - y);
      rows.forEach((r, i) => r.classList.toggle('sel', i >= lo && i <= hi));
    } else if (e.ctrlKey || e.metaKey) { row.classList.toggle('sel'); S.anchorRow = row; } // one more
    else { $$('.row.sel').forEach(r => r.classList.remove('sel')); row.classList.add('sel'); S.anchorRow = row; }
    return;
  }
  const hrefEl = t.closest('[data-href]');
  if (hrefEl && hrefEl.dataset.href) { go(hrefEl.dataset.href); return; }
  const webEl = t.closest('.card[data-kind="web"]'); if (webEl) { playPv(webEl.dataset.key, webEl); return; }
  const cardEl = t.closest('.card[data-kind="track"]');
  if (cardEl) { playFrom(cardEl.dataset.key); return; }
  const lnk = t.closest('a[href^="#/"]'); if (lnk) return; // default navigation
});

function playFrom(id) {
  // Play a lone track with its neighbours on the current page as the context.
  const ctx = $$('.row', $('#view')).map(r => r.dataset.id).filter(Boolean);
  if (ctx.includes(id)) setContext(ctx, ctx.indexOf(id), { label: sourceLabelForRoute() });
  else setContext([id], 0, { shuffle: false, label: trackOf(id).title });
}
function sourceLabelForRoute() {
  return { liked: 'Liked Songs', library: 'Your songs', queue: 'Queue', search: 'Search', artist: route.arg, playlist: S.playlists.find(p => p.id === route.arg)?.name, mix: sourceLabel('mix:' + route.arg) }[route.name] || 'Your library';
}

document.addEventListener('dblclick', e => {
  const row = e.target.closest('.row'); if (!row || e.target.closest('button,a')) return;
  const qbox = row.parentElement?.dataset.sort;
  if (qbox?.startsWith('queue-')) { queueJump(qbox.slice(6), +row.dataset.i); return; }
  const ids = S.lists[row.dataset.list] || [row.dataset.id];
  setContext(ids, +row.dataset.i, { label: sourceLabelForRoute() });
});
document.addEventListener('click', e => { // the number cell's play glyph
  const p = e.target.closest('.row .num'); if (!p) return;
  const row = p.closest('.row'); const ids = S.lists[row.dataset.list] || [row.dataset.id];
  const qbox = row.parentElement?.dataset.sort;
  if (qbox?.startsWith('queue-')) { queueJump(qbox.slice(6), +row.dataset.i); return; }
  if (P.cur?.id === row.dataset.id) togglePlay(); else setContext(ids, +row.dataset.i, { label: sourceLabelForRoute() });
});
document.addEventListener('keydown', e => {
  if (e.key === 'Enter' && e.target.matches('.card,.qtile,.tr-card') && !e.target.matches('button')) { e.target.click(); return; }
  if (e.target.matches('input,textarea,select') || e.ctrlKey && !['ArrowRight', 'ArrowLeft'].includes(e.key) || e.altKey) return;
  if (e.key === ' ' && !e.target.matches('button')) { e.preventDefault(); togglePlay(); }
  else if (e.key === 'ArrowRight' && e.ctrlKey) next(false);
  else if (e.key === 'ArrowLeft' && e.ctrlKey) prev();
  else if (e.key === 'ArrowRight') audio.currentTime = Math.min(audio.duration || 0, audio.currentTime + 5);
  else if (e.key === 'ArrowLeft') audio.currentTime = Math.max(0, audio.currentTime - 5);
  else if (e.key.toLowerCase() === 'l' && P.cur) toggleLike(P.cur.id);
  else if (e.key.toLowerCase() === 's') toggleShuffle();
  else if (e.key === '/') { e.preventDefault(); $('#q').focus(); }
});
document.addEventListener('contextmenu', e => {
  const row = e.target.closest('.row'); if (row?.dataset.id) { e.preventDefault(); trackMenu(row.dataset.id, e.clientX, e.clientY, route.name === 'playlist' ? route.arg : null); return; }
  const li = e.target.closest('.lib-item[data-pl]'); if (li) { e.preventDefault(); playlistMenu(li.dataset.pl, e.clientX, e.clientY); return; }
  const cd = e.target.closest('.card[data-kind]');
  if (cd && ['album', 'mix', 'artist', 'playlist'].includes(cd.dataset.kind)) {
    e.preventDefault();
    if (cd.dataset.kind === 'playlist') playlistMenu(cd.dataset.key, e.clientX, e.clientY);
    else sourceMenu(`${cd.dataset.kind}:${cd.dataset.key}`, e.clientX, e.clientY);
  }
});
document.addEventListener('mouseover', e => { // seek bar gets the accent while dragging too
  if (e.target === $('#seek')) $('#seek').style.setProperty('--p', $('#seek').value / 10 + '%');
});

/* -------------------------------------------------------------------- boot */

/* Desktop window: the native title bar is off, so the page draws its own. window.ipc exists only
   inside the desktop app; in a browser the bar stays hidden. */
if (window.ipc) {
  document.body.classList.add('desktop');
  const send = m => window.ipc.postMessage(m);
  document.addEventListener('mousedown', e => {
    if (e.button !== 0 || !e.target.closest('[data-win="drag"]')) return;
    send(e.detail === 2 ? 'max' : 'drag'); // double-click toggles maximize
  });
  $$('.wincontrols [data-win]').forEach(b => { b.onclick = () => send(b.dataset.win); });
  const syncMaxed = () => document.body.classList.toggle('maxed', outerWidth >= screen.availWidth - 2 && outerHeight >= screen.availHeight - 2);
  addEventListener('resize', syncMaxed); syncMaxed();
}

/* Keyboard media keys, also while Vinyl is in the background. The desktop app grabs them
   system-wide and the server hands the presses over; in a browser this does nothing. */
async function mediaKeyLoop() {
  let idle = 0, errors = 0;
  for (;;) {
    try {
      const r = await api('/api/media/poll');
      errors = 0;
      if (!r.enabled) { if (++idle >= 3) return; await new Promise(res => setTimeout(res, 2500)); continue; }
      idle = 0;
      for (const k of r.keys) {
        if (k === 'play_pause') togglePlay();
        else if (k === 'next') next(false);
        else if (k === 'prev') prev();
        else if (k === 'stop') audio.pause();
      }
    } catch { if (++errors >= 10) return; await new Promise(res => setTimeout(res, 3000)); }
  }
}

const skeletonHTML = () => `<div class="skel"><div class="skel-title"></div><div class="skel-row">${'<div class="skel-card"></div>'.repeat(4)}</div><div class="skel-title small"></div><div class="skel-row">${'<div class="skel-card"></div>'.repeat(6)}</div></div>`;

(async function boot() {
  $('#b-play').innerHTML = ICON.play;
  document.body.classList.add('paused');
  if (S.recs?.items) stash(S.recs.items);
  Object.values(S.mixNews).forEach(stash);

  // 1. Paint at once from what was on screen last time (the server may take a moment to answer).
  const cached = lsGet('vinyl.data', null);
  let painted = false;
  const firstPaint = state => {
    restoreFrom(state);
    renderRoute(false);
    renderBarTrack();
    applyNPState();
    painted = true;
  };
  if (cached?.tracks) { applyData(cached); firstPaint(lsGet('vinyl.state', null)); }
  else $('#view').innerHTML = skeletonHTML();

  // 2. Ask the server for the real data and the saved player state, in parallel.
  let fresh, serverState = null;
  try { [fresh, serverState] = await Promise.all([fetchData(), api('/api/state').catch(() => null)]); }
  catch (e) { if (!painted) $('#view').innerHTML = `<div class="empty"><b>Can't reach the Vinyl server</b>${esc(e.message)}</div>`; return; }
  lsSet('vinyl.data', fresh);
  if (!painted) { applyData(fresh); firstPaint(serverState || lsGet('vinyl.state', null)); }
  else if (dataSig(fresh) !== dataSig(cached)) { applyData(fresh); renderLibrary(); renderRoute(true); renderBarTrack(); }
  else applyData(fresh); // same on screen: just refresh the numbers (plays, durations) without redrawing

  mediaKeyLoop();
  setTimeout(() => checkForUpdate(), 2500); // ask GitHub whether there is a newer release
  setInterval(() => { // pick up songs added outside the app (downloads finishing, new files)
    if (document.hidden) return;
    api('/api/library').then(ts => { const withAlbum = x => x.filter(t => t.album).length; if (ts.length !== S.tracks.length || withAlbum(ts) !== withAlbum(S.tracks)) loadAll().then(() => { renderLibrary(); if (['home', 'library', 'albums', 'album'].includes(route.name)) renderRoute(true); }); }).catch(() => { });
  }, 30000);
})();
