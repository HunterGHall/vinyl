// ts was made by claude

'use strict';
/* The now-playing record: a turntable disc rendered as live ASCII (ported from ascii-vinyl).
   The cover sits on the label; the disc turns while music plays.
   Defaults are the settings picked in ascii-vinyl: rotate, 1.10 rad/s, clockwise, fixed light
   at 163 degrees, intensity 1.00, cover 300 px / radius 120 px, 5 px cells, classic characters,
   original colours. */


const AsciiVinyl = (() => {
  const S = 1080, CX = 540, CY = 540, R_DISC = 440, F = 720, K = F / S;
  const out = document.getElementById('ascii');
  const octx = out.getContext('2d');
  const DEFAULTS = { spin: true, speed: 1.1, dir: '1', sheen: 'fixed', light: 163, glow: 1, cover: 300, rad: 120, cell: 5, ramp: 'classic', color: 'orig' };
  let cfg = { ...DEFAULTS };
  const save = () => { dirty = true; try { localStorage.setItem('vinyl.ascii', JSON.stringify(cfg)); } catch { /* private mode */ } };

  const RAMPS = { classic: ' .:-=+*#%@', dense: ' .,:;i1tfLCG08@', blocks: ' ░▒▓█', binary: ' 01' };
  const MONO = { white: [235, 235, 235], green: [70, 255, 120], amber: [255, 170, 40] };

  const src = document.createElement('canvas'); src.width = src.height = F;
  const sctx = src.getContext('2d', { willReadFrequently: true });

  // seeded random so the scuffs stay put on the disc
  let seed = 7; const rnd = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const scuffs = [], rings = [], marks = [];
  for (let i = 0; i < 70; i++) scuffs.push({ r: 190 + rnd() * 235, a: rnd() * 6.283, len: 0.08 + rnd() * 0.4, w: 1.5 + rnd() * 3, al: 0.10 + rnd() * 0.22 });
  for (let r = 175; r < R_DISC - 6; r += 6) rings.push({ r, a: 0.05 + rnd() * 0.18 });
  for (let i = 0; i < 200; i++) marks.push({ r: 180 + rnd() * 250, a: rnd() * 6.283, len: 0.05 + rnd() * 0.5, w: 2 + rnd() * 5, al: 0.15 + rnd() * 0.3 });
  const gaps = [235, 285, 340, 392];
  const stops = [[0, 0], [0.07, 0.05], [0.17, 0.62], [0.25, 0.36], [0.33, 0.05], [0.48, 0], [0.60, 0.08], [0.70, 0.62], [0.77, 0.85], [0.85, 0.30], [0.93, 0.05], [1, 0]];

  let cover = null; // HTMLImageElement of the current artwork (null -> the built-in label art)
  let coverUrl, coverPending = false, pendingSince = 0, drawn = false;
  // While the song is paused (or the disc isn't set to turn) the record holds still, so a frame is only
  // drawn when something on it changes: a new cover, a settings tweak, or reopening Now Playing.
  let dirty = true;
  // The record is not drawn while the right cover is still loading, and the previous cover stays on it
  // until the new one is ready: the built-in label art only ever shows for songs that have no cover.
  function setCover(url) {
    url = url || null;
    if (url === coverUrl) return;
    coverUrl = url;
    dirty = true;
    if (!url) { cover = null; coverPending = false; return; }
    coverPending = true; pendingSince = performance.now();
    const img = new Image();
    img.crossOrigin = 'anonymous';
    const done = ok => { if (coverUrl !== url) return; cover = ok && probe(img) ? img : null; coverPending = false; dirty = true; };
    img.onload = () => done(true);
    img.onerror = () => done(false);
    img.src = url;
  }
  function probe(img) { // cross-origin art without CORS headers would taint the canvas
    try { const c = document.createElement('canvas'); c.width = c.height = 2; const x = c.getContext('2d'); x.drawImage(img, 0, 0, 2, 2); x.getImageData(0, 0, 1, 1); return true; } catch { return false; }
  }

  function drawSheen(c, start) {
    const g = c.createConicGradient(start, 0, 0);
    for (const [p, a] of stops) g.addColorStop(p, `rgba(255,255,255,${Math.min(1, a * cfg.glow)})`);
    c.globalCompositeOperation = 'lighter';
    c.fillStyle = g; c.fillRect(-S, -S, 2 * S, 2 * S);
    c.globalCompositeOperation = 'source-over';
  }

  function drawCover(c) {
    const s = cfg.cover, r = Math.min(cfg.rad, s / 2), x = CX - s / 2, y = CY - s / 2;
    c.fillStyle = 'rgba(0,0,0,0.9)';
    c.beginPath(); c.roundRect(x - 14, y - 14, s + 28, s + 28, r + 14); c.fill();
    c.save();
    c.beginPath(); c.roundRect(x, y, s, s, r); c.clip();
    if (cover) {
      const w = cover.naturalWidth || 1, h = cover.naturalHeight || 1, m = Math.min(w, h);
      c.drawImage(cover, (w - m) / 2, (h - m) / 2, m, m, x, y, s, s);
    } else {
      const u = s / 300;
      c.fillStyle = '#f2edc0'; c.fillRect(x, y, s, s);
      c.fillStyle = '#e8641e'; c.fillRect(x, y + s * 0.66, s, s * 0.34);
      c.fillStyle = '#1a1612'; c.beginPath(); c.arc(CX, y + s * 0.44, 82 * u, 0, 6.2832); c.fill();
      c.fillStyle = '#f2edc0'; c.beginPath(); c.arc(CX, y + s * 0.44, 56 * u, 0, 6.2832); c.fill();
      c.fillStyle = '#e8641e'; c.beginPath(); c.arc(CX, y + s * 0.44, 30 * u, 0, 6.2832); c.fill();
      c.fillStyle = '#1a1612';
      for (let i = 0; i < 3; i++) c.fillRect(x + 36 * u, y + s * 0.74 + i * 16 * u, (180 - i * 50) * u, 7 * u);
      c.fillRect(x + 36 * u, y + 30 * u, 70 * u, 10 * u);
    }
    c.restore();
    c.strokeStyle = 'rgba(26,22,18,0.9)'; c.lineWidth = 3 * (s / 300);
    c.beginPath(); c.roundRect(x + 5 * (s / 300), y + 5 * (s / 300), s - 10 * (s / 300), s - 10 * (s / 300), Math.max(0, r - 5 * (s / 300))); c.stroke();
  }

  function drawSource(ang) {
    const c = sctx;
    c.setTransform(K, 0, 0, K, 0, 0);
    c.globalCompositeOperation = 'source-over';
    c.fillStyle = '#000'; c.fillRect(0, 0, S, S);
    c.save();
    c.translate(CX, CY);
    c.beginPath(); c.arc(0, 0, R_DISC, 0, 6.2832); c.clip();
    c.fillStyle = '#0b0b0b'; c.fillRect(-S, -S, 2 * S, 2 * S);
    const start = -Math.PI / 2 + cfg.light * Math.PI / 180;
    if (cfg.sheen !== 'disc') { c.save(); drawSheen(c, start); c.restore(); }
    c.save();
    c.rotate(ang);
    if (cfg.sheen === 'disc') drawSheen(c, start);
    c.lineWidth = 2;
    for (const g of rings) { c.strokeStyle = `rgba(0,0,0,${g.a})`; c.beginPath(); c.arc(0, 0, g.r, 0, 6.2832); c.stroke(); }
    c.lineCap = 'round';
    for (const m of marks) { c.strokeStyle = `rgba(0,0,0,${m.al})`; c.lineWidth = m.w; c.beginPath(); c.arc(0, 0, m.r, m.a, m.a + m.len); c.stroke(); }
    c.globalCompositeOperation = 'lighter';
    for (const s of scuffs) { c.strokeStyle = `rgba(255,255,255,${s.al})`; c.lineWidth = s.w; c.beginPath(); c.arc(0, 0, s.r, s.a, s.a + s.len); c.stroke(); }
    c.globalCompositeOperation = 'source-over';
    c.restore();
    c.lineWidth = 9;
    for (const r of gaps) { c.strokeStyle = 'rgba(0,0,0,0.55)'; c.beginPath(); c.arc(0, 0, r, 0, 6.2832); c.stroke(); }
    const rg = c.createRadialGradient(0, 0, 150, 0, 0, R_DISC);
    rg.addColorStop(0, 'rgba(0,0,0,0.80)'); rg.addColorStop(0.35, 'rgba(0,0,0,0.20)'); rg.addColorStop(0.85, 'rgba(0,0,0,0.10)'); rg.addColorStop(1, 'rgba(0,0,0,0.60)');
    c.fillStyle = rg; c.fillRect(-S, -S, 2 * S, 2 * S);
    c.restore();
    drawCover(c);
    return c.getImageData(0, 0, F, F);
  }

  let cw, ch, cols, rows, gctx;
  function layout() {
    cw = cfg.cell; ch = Math.round(cw * 1.8);
    cols = Math.ceil(S / cw); rows = Math.ceil(S / ch);
    const grid = document.createElement('canvas'); grid.width = cols; grid.height = rows;
    gctx = grid.getContext('2d', { willReadFrequently: true });
    gctx.imageSmoothingEnabled = true; gctx.imageSmoothingQuality = 'high';
    dirty = true;
  }
  layout();

  let tPrev = performance.now(), ang = 0, running = false, isPlaying = () => false, lastDraw = 0;
  function frame(now) {
    if (!running) return;
    requestAnimationFrame(frame);
    if (now - lastDraw < 30) return; // ~30 fps is plenty for ASCII and keeps the CPU calm
    if (document.hidden) { tPrev = now; return; } // minimised or behind another window: nothing to show
    if (coverPending && now - pendingSince < 1500) return; // wait for the cover instead of showing the wrong art
    const turning = cfg.spin && isPlaying();
    if (!turning && !dirty) { tPrev = now; return; } // standing still and unchanged: skip the work
    dirty = false;
    lastDraw = now;
    const dt = Math.min(0.1, (now - tPrev) / 1000); tPrev = now;
    if (turning) ang += dt * cfg.speed * +cfg.dir;
    drawSource(ang);
    gctx.drawImage(src, 0, 0, cols, rows);
    const d = gctx.getImageData(0, 0, cols, rows).data;
    octx.clearRect(0, 0, S, S);
    octx.font = `${Math.round(ch * 0.95)}px ui-monospace, Consolas, "Courier New", monospace`;
    octx.textAlign = 'center'; octx.textBaseline = 'middle';
    const ramp = RAMPS[cfg.ramp], L = ramp.length, mono = MONO[cfg.color];
    for (let j = 0; j < rows; j++) {
      for (let i = 0; i < cols; i++) {
        const o = (j * cols + i) * 4, r = d[o], g = d[o + 1], b = d[o + 2], m = Math.max(r, g, b);
        if (m < 14) continue;
        let lum = Math.pow(m / 255, 0.75) * 1.1 + (Math.random() - 0.5) * 0.12; // shimmer
        const idx = Math.min(L - 1, Math.max(0, (lum * L) | 0));
        if (idx === 0) continue;
        const f = Math.min(1, 0.3 + lum * 0.9);
        let cr, cg, cb;
        if (mono) { cr = mono[0] * f; cg = mono[1] * f; cb = mono[2] * f; }
        else { const n = 255 / m; cr = r * n * f; cg = g * n * f; cb = b * n * f; }
        octx.fillStyle = `rgb(${cr | 0},${cg | 0},${cb | 0})`;
        octx.fillText(ramp[idx], i * cw + cw / 2, j * ch + ch / 2);
      }
    }
    if (!drawn) { drawn = true; out.classList.add('ready'); } // fade the record in once it has its cover
  }

  function start(playingFn) { isPlaying = playingFn; if (running) return; running = true; dirty = true; tPrev = performance.now(); requestAnimationFrame(frame); }
  function stop() { running = false; }

  // ---- settings panel (same controls as ascii-vinyl) ----
  const fmts = { light: v => v + '°', glow: v => (+v).toFixed(2) + '×', speed: v => (+v).toFixed(2) + ' rad/s', cover: v => v + 'px', rad: v => v + 'px', cell: v => v + 'px' };
  const FIELDS = [
    ['h', 'Outer disc'],
    ['check', 'spin', 'Rotate'],
    ['range', 'speed', 'Speed', 0.05, 3, 0.05],
    ['select', 'dir', 'Direction', [['1', 'Clockwise'], ['-1', 'Counter-clockwise']]],
    ['select', 'sheen', 'Sheen', [['fixed', 'Fixed light'], ['disc', 'Turns with disc']]],
    ['h', 'Lighting'],
    ['range', 'light', 'Light angle', 0, 360, 1],
    ['range', 'glow', 'Intensity', 0.2, 1.6, 0.05],
    ['h', 'Cover'],
    ['range', 'cover', 'Size', 200, 380, 5],
    ['range', 'rad', 'Corner radius', 0, 120, 2],
    ['h', 'ASCII'],
    ['range', 'cell', 'Cell size', 5, 16, 1],
    ['select', 'ramp', 'Characters', [['classic', 'Classic  .:-=+*#%@'], ['dense', 'Dense  .,:;i1tfLCG08@'], ['blocks', 'Blocks  ░▒▓█'], ['binary', 'Binary  01']]],
    ['select', 'color', 'Colour', [['orig', 'Original'], ['white', 'Mono white'], ['green', 'Mono green'], ['amber', 'Mono amber']]],
  ];
  function buildPanel(box) {
    box.innerHTML = FIELDS.map(f => {
      if (f[0] === 'h') return `<h4>${f[1]}</h4>`;
      const [type, key, label] = f;
      if (type === 'check') return `<label class="check">${label}<input type="checkbox" data-k="${key}" ${cfg[key] ? 'checked' : ''}></label>`;
      if (type === 'range') return `<label><span class="row">${label}<b data-v="${key}">${fmts[key](cfg[key])}</b></span><input type="range" data-k="${key}" min="${f[3]}" max="${f[4]}" step="${f[5]}" value="${cfg[key]}"></label>`;
      return `<label>${label}<select data-k="${key}">${f[3].map(([v, t]) => `<option value="${v}" ${String(cfg[key]) === v ? 'selected' : ''}>${t}</option>`).join('')}</select></label>`;
    }).join('') + '<button class="reset" type="button">Reset to defaults</button>';
    box.addEventListener('input', e => {
      const el = e.target.closest('[data-k]'); if (!el) return;
      const k = el.dataset.k;
      cfg[k] = el.type === 'checkbox' ? el.checked : el.type === 'range' ? +el.value : el.value;
      const v = box.querySelector(`[data-v="${k}"]`); if (v) v.textContent = fmts[k](el.value);
      if (k === 'cell') layout();
      save();
    });
    box.querySelector('.reset').onclick = () => { cfg = { ...DEFAULTS }; save(); layout(); buildPanelInner(box); };
  }
  function buildPanelInner(box) { const c = box.cloneNode(false); box.replaceWith(c); c.id = box.id; buildPanel(c); }

  return { start, stop, setCover };
})();
