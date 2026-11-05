// screen-mark website: the first screen runs under a working copy of the app's capture overlay.
// Selection, size pill, toolbar placement and annotation shapes follow src/overlay and src/tools.
(() => {
  const stage = document.querySelector('[data-stage]');
  if (!stage) return;
  const q = (s) => stage.querySelector(s);
  const capture = q('[data-capture]'), sel = q('.sel'), pill = q('.pill'), bar = q('[data-toolbar]');
  const ink = q('.ink'), cross = q('.cross'), toast = q('.toast');
  const fine = matchMedia('(hover: hover) and (pointer: fine)').matches;
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const NS = 'http://www.w3.org/2000/svg';
  const AMBER = 'var(--rfs-accent)';

  let rect = null, initial = true, tool = 'arrow', color = AMBER, shapes = [], undone = [];
  let drag = null, toastTimer = 0;

  const box = () => stage.getBoundingClientRect();
  const at = (e) => { const b = box(); return { x: e.clientX - b.left, y: e.clientY - b.top }; };
  const of = (el) => { const b = box(), r = el.getBoundingClientRect(); return { x: r.left - b.left, y: r.top - b.top, w: r.width, h: r.height }; };
  const inside = (p) => rect && p.x >= rect.x && p.x <= rect.x + rect.w && p.y >= rect.y && p.y <= rect.y + rect.h;
  const px = (n) => Math.round(n * (window.devicePixelRatio || 1));

  // --- Selection, size pill and toolbar, placed the way the app places them ---
  function setSel(r, withBar = true) {
    rect = r;
    stage.classList.toggle('no-sel', !r);
    stage.classList.toggle('has-bar', !!r && withBar);
    if (!r) { sel.style.width = sel.style.height = '0px'; render(); return; }
    sel.style.transform = `translate(${r.x}px, ${r.y}px)`;
    sel.style.width = r.w + 'px';
    sel.style.height = r.h + 'px';
    pill.textContent = `${px(r.w)} × ${px(r.h)}`;
    const W = stage.clientWidth, H = stage.clientHeight, pw = pill.offsetWidth, ph = pill.offsetHeight;
    let bx = 0, by = 0;
    if (withBar) {
      const bw = bar.offsetWidth;
      bx = Math.max(16, Math.min(r.x + (r.w - bw) / 2, W - bw - 16));
      by = r.y + r.h + 8;
      if (by + 40 > H) by = Math.max(0, r.y - 48);
      const op = (bar.offsetParent || stage).getBoundingClientRect(), sb = box();
      bar.style.left = bx - (op.left - sb.left) + 'px';
      bar.style.top = by - (op.top - sb.top) + 'px';
    }
    // Bottom-right outside the selection as in the app; then above it; then just inside its corner
    let x = r.x + r.w - pw + 4;
    if (x + pw > W) x = r.x - 4;
    const bw = bar.offsetWidth, hits = (y) => withBar && y < by + 40 && y + ph > by && x < bx + bw && x + pw > bx;
    const y = [r.y + r.h + 4, r.y - ph - 4].find((c) => c >= 0 && c + ph <= H && !hits(c)) ?? r.y + r.h - ph - 4;
    pill.style.transform = `translate(${Math.max(0, x)}px, ${Math.max(0, y)}px)`;
    render();
  }

  // --- Annotations, drawn as SVG with the app's geometry ---
  const el = (name, attrs) => { const n = document.createElementNS(NS, name); for (const k in attrs) n.setAttribute(k, attrs[k]); return n; };
  function head(a, b, t) {
    const dx = b.x - a.x, dy = b.y - a.y, len = Math.hypot(dx, dy) || 1, ux = dx / len, uy = dy / len;
    const back = 4 * t * Math.cos(Math.PI / 6), side = 4 * t * Math.sin(Math.PI / 6), cx = b.x - ux * back, cy = b.y - uy * back;
    return { base: { x: cx, y: cy }, pts: `${b.x},${b.y} ${cx - uy * side},${cy + ux * side} ${cx + uy * side},${cy - ux * side}` };
  }
  function shape(s) {
    const st = `stroke:${s.color};fill:none;stroke-width:3;stroke-linecap:round;stroke-linejoin:round`;
    const x = Math.min(s.a.x, s.b.x), y = Math.min(s.a.y, s.b.y), w = Math.abs(s.b.x - s.a.x), h = Math.abs(s.b.y - s.a.y);
    const g = el('g', {});
    if (s.kind === 'arrow') {
      const hd = head(s.a, s.b, 3);
      g.append(el('line', { x1: s.a.x, y1: s.a.y, x2: hd.base.x, y2: hd.base.y, style: st }), el('polygon', { points: hd.pts, style: `fill:${s.color}` }));
    } else if (s.kind === 'rect' || s.kind === 'rounded') {
      g.append(el('rect', { x, y, width: w, height: h, rx: s.kind === 'rounded' ? 10 : 0, style: st }));
    } else if (s.kind === 'circle') {
      g.append(el('ellipse', { cx: x + w / 2, cy: y + h / 2, rx: w / 2, ry: h / 2, style: st }));
    } else if (s.kind === 'line') {
      g.append(el('line', { x1: s.a.x, y1: s.a.y, x2: s.b.x, y2: s.b.y, style: st }));
    } else if (s.kind === 'pencil') {
      g.append(el('polyline', { points: s.pts.map((p) => p.x + ',' + p.y).join(' '), style: st }));
    } else if (s.kind === 'highlight') {
      g.append(el('rect', { x, y, width: w, height: h, style: `fill:${s.color};mix-blend-mode:multiply` }));
    } else if (s.kind === 'measure') {
      const len = Math.hypot(s.b.x - s.a.x, s.b.y - s.a.y) || 1, nx = -(s.b.y - s.a.y) / len * 7, ny = (s.b.x - s.a.x) / len * 7;
      const m = `stroke:${s.color};stroke-width:2;stroke-linecap:round`;
      g.append(el('line', { x1: s.a.x, y1: s.a.y, x2: s.b.x, y2: s.b.y, style: m }));
      for (const p of [s.a, s.b]) g.append(el('line', { x1: p.x + nx, y1: p.y + ny, x2: p.x - nx, y2: p.y - ny, style: m }));
      const label = el('text', { x: (s.a.x + s.b.x) / 2 + nx * 1.6, y: (s.a.y + s.b.y) / 2 + ny * 1.6, class: 'ink-label', 'text-anchor': 'middle', 'dominant-baseline': 'middle' });
      label.textContent = px(len) + ' px';
      g.append(label);
    } else if (s.kind === 'step') {
      g.append(el('circle', { cx: s.a.x, cy: s.a.y, r: 15, style: `fill:${s.color};stroke:#fff;stroke-width:2` }));
      const n = el('text', { x: s.a.x, y: s.a.y + 0.5, 'text-anchor': 'middle', 'dominant-baseline': 'central', class: 'ink-step', style: s.color === AMBER ? '' : 'fill:#0C0D10' });
      n.textContent = s.n;
      g.append(n);
    }
    return g;
  }
  function render() {
    ink.replaceChildren();
    if (!rect) return;
    const clip = el('clipPath', { id: 'sel-clip' });
    clip.append(el('rect', { x: rect.x, y: rect.y, width: rect.w, height: rect.h }));
    const g = el('g', { 'clip-path': 'url(#sel-clip)' });
    const lights = shapes.filter((s) => s.kind === 'spotlight');
    if (lights.length) {
      let d = `M${rect.x} ${rect.y}h${rect.w}v${rect.h}h${-rect.w}z`;
      for (const s of lights) d += `M${Math.min(s.a.x, s.b.x)} ${Math.min(s.a.y, s.b.y)}h${Math.abs(s.b.x - s.a.x)}v${Math.abs(s.b.y - s.a.y)}h${-Math.abs(s.b.x - s.a.x)}z`;
      g.append(el('path', { d, 'fill-rule': 'evenodd', style: 'fill:rgb(0 0 0 / .5)' }));
    }
    for (const s of shapes) if (s.kind !== 'spotlight') g.append(shape(s));
    ink.append(el('defs', {}), g);
    ink.firstChild.append(clip);
  }

  // --- The corner popup ---
  function say(title, detail, sticky = false) {
    toast.querySelector('.toast__title').textContent = title;
    toast.querySelector('.toast__detail').textContent = detail;
    toast.classList.remove('is-shown');
    void toast.offsetWidth;
    toast.classList.add('is-shown');
    clearTimeout(toastTimer);
    if (!sticky) toastTimer = setTimeout(() => toast.classList.remove('is-shown'), 5000);
  }
  const size = () => (rect ? `${px(rect.w)} × ${px(rect.h)}` : 'the selection');
  const stamp = () => {
    const d = new Date(), p = (n) => String(n).padStart(2, '0');
    return `screen-mark_${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}_${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}.png`;
  };
  const ACTS = {
    copy: () => say('Nothing copied', `This is a web page. The app would copy ${size()} with what you drew on it.`),
    save: () => say('Nothing saved', `The app would have written
${stamp()}`),
    pin: () => say('Pin needs the app', 'It keeps the capture on top of every window.'),
    upload: () => say('Upload needs the app', 'It sends the capture to Imgur anonymously, with your own client ID.'),
    ocr: () => say('OCR needs the app', 'It reads the capture with Windows OCR and copies the text.'),
  };
  const PIXELS = { text: 'Text', pixelate: 'Pixelate', eyedropper: 'Eyedropper' };

  // --- Tools and colors ---
  const KEYS = { v: 'select', a: 'arrow', r: 'rect', c: 'circle', o: 'rounded', l: 'line', p: 'pencil', h: 'highlight', f: 'spotlight', t: 'text', b: 'pixelate', n: 'step', i: 'eyedropper', m: 'measure' };
  const keyOf = Object.fromEntries(Object.entries(KEYS).map(([k, v]) => [v, k]));
  function pick(name) {
    tool = name;
    bar.querySelectorAll('[data-tool]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.tool === name)));
    document.querySelectorAll('.key.is-tool').forEach((k) => k.classList.toggle('is-current', k.dataset.key === keyOf[name]));
  }
  bar.addEventListener('click', (e) => {
    const b = e.target.closest('button');
    if (!b) return;
    if (b.dataset.tool) pick(b.dataset.tool);
    if (b.dataset.color) {
      color = b.dataset.color === 'amber' ? AMBER : b.dataset.color;
      bar.querySelectorAll('[data-color]').forEach((s) => s.setAttribute('aria-pressed', String(s === b)));
    }
    if (b.dataset.act) ACTS[b.dataset.act]();
  });
  // One tab stop for the toolbar; arrow keys move along it
  const btns = [...bar.querySelectorAll('button')];
  btns.forEach((b, i) => (b.tabIndex = i === 1 ? 0 : -1));
  bar.addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    const vis = btns.filter((b) => b.offsetParent);
    const i = vis.indexOf(document.activeElement), n = vis[(i + (e.key === 'ArrowRight' ? 1 : vis.length - 1)) % vis.length];
    btns.forEach((b) => (b.tabIndex = -1));
    n.tabIndex = 0;
    n.focus();
    e.preventDefault();
  });

  // --- Pointer: drag to select, click to snap to a block, draw inside the selection ---
  function clearAll() { shapes = []; undone = []; stage.classList.add('is-clean'); }
  document.addEventListener('pointerdown', (e) => {
    if (!fine || e.button !== 0 || !stage.classList.contains('is-ready') || e.target.closest('a, button, .toast')) return;
    const p = at(e);
    if (p.y < 0 || p.y > stage.clientHeight) return;
    e.preventDefault();
    stage.setPointerCapture(e.pointerId);
    if (inside(p) && tool !== 'select') {
      if (PIXELS[tool]) { say(`${PIXELS[tool]} works in the app`, 'It needs the captured pixels, which a web page doesn’t have. Shapes work here.'); return; }
      const statics = stage.classList.contains('is-clean') ? 0 : 1;
      const s = { kind: tool, a: p, b: p, pts: [p], color, n: shapes.filter((x) => x.kind === 'step').length + 1 + statics };
      shapes.push(s);
      undone = [];
      drag = { mode: 'draw', s };
      render();
    } else if (!inside(p)) {
      drag = { mode: 'select', a: p, moved: false };
      stage.classList.add('is-crosshair');
    }
  });
  stage.addEventListener('pointermove', (e) => {
    if (!fine) return;
    const p = at(e);
    cross.style.setProperty('--cx', p.x + 'px');
    cross.style.setProperty('--cy', p.y + 'px');
    if (!drag) return;
    if (drag.mode === 'draw') {
      drag.s.b = p;
      if (drag.s.kind === 'pencil') drag.s.pts.push(p);
      render();
    } else {
      if (Math.abs(p.x - drag.a.x) + Math.abs(p.y - drag.a.y) > 4 && !drag.moved) { drag.moved = true; initial = false; clearAll(); }
      if (drag.moved) setSel({ x: Math.min(drag.a.x, p.x), y: Math.min(drag.a.y, p.y), w: Math.abs(p.x - drag.a.x), h: Math.abs(p.y - drag.a.y) }, false);
    }
  });
  stage.addEventListener('pointerup', (e) => {
    if (!drag) return;
    if (drag.mode === 'select') {
      stage.classList.toggle('is-crosshair', !rect && !drag.moved);
      if (drag.moved) setSel(rect);
      else {
        // A click selects the block under the pointer, like clicking a window in the app
        const win = document.elementFromPoint(e.clientX, e.clientY)?.closest('[data-window]');
        if (win) { initial = win === capture; clearAll(); const r = of(win); setSel({ x: r.x - 8, y: r.y - 8, w: r.w + 16, h: r.h + 16 }); }
      }
      stage.classList.toggle('is-crosshair', !rect);
    } else if (drag.s.kind !== 'step' && drag.s.kind !== 'pencil' && Math.hypot(drag.s.b.x - drag.s.a.x, drag.s.b.y - drag.s.a.y) < 3) {
      shapes.pop();
      render();
    }
    drag = null;
  });

  // --- Keys: the app's tool keys, undo and redo, Enter to copy, Esc to start over ---
  addEventListener('keydown', (e) => {
    const k = e.key.toLowerCase();
    if (e.ctrlKey && k === 'z') {
      if (!shapes.length && !undone.length) return;
      if (e.shiftKey) { if (undone.length) shapes.push(undone.pop()); } else if (shapes.length) undone.push(shapes.pop());
      render();
      e.preventDefault();
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey || e.repeat) return;
    if (k === 'escape' && rect) {
      initial = false;
      clearAll();
      setSel(null);
      stage.classList.toggle('is-crosshair', fine);
      say('Overlay cleared', fine ? 'Drag anywhere on this screen for a new selection, or click a block to select it.' : 'Reload the page to see the selection again.');
    } else if (k === 'enter' && rect && document.activeElement === document.body) {
      ACTS.copy();
    } else if (KEYS[k]) {
      pick(KEYS[k]);
      const key = document.querySelector(`.key[data-key="${k}"]`);
      if (key) { key.classList.add('is-down'); setTimeout(() => key.classList.remove('is-down'), 140); }
    }
  });

  // --- The highlighter mark in the quote draws in once, when it's on screen ---
  const mark = document.querySelector('.why mark');
  if (mark && 'IntersectionObserver' in window) {
    const io = new IntersectionObserver(([en]) => { if (en.isIntersecting) { mark.classList.add('is-drawn'); io.disconnect(); } }, { rootMargin: '0px 0px -30% 0px' });
    io.observe(mark);
  } else if (mark) mark.classList.add('is-drawn');

  // --- Copy button in Install ---
  document.querySelectorAll('[data-copy]').forEach((b) => b.addEventListener('click', () => {
    navigator.clipboard?.writeText(b.dataset.copy).then(() => {
      b.textContent = 'Copied';
      setTimeout(() => (b.textContent = 'Copy'), 2000);
    });
  }));

  // --- Keep the first selection on the download block when the layout changes ---
  let rz = 0;
  addEventListener('resize', () => {
    cancelAnimationFrame(rz);
    rz = requestAnimationFrame(() => { if (initial && rect) setSel(of(capture)); else if (rect) setSel(rect); });
  });

  // --- Entrance: dim, crosshair to the corner, drag the selection open, then the toolbar and notes ---
  function ready() {
    stage.classList.remove('is-crosshair');
    stage.classList.add('is-ready');
    if (fine) {
      stage.classList.add('is-live');
      say('This page is under the overlay', 'Drag anywhere to select, then draw inside it. A, R, C, L, P, N and M pick tools; Esc starts over.', true);
    }
  }
  function start() {
    pick('arrow');
    const t = of(capture);
    stage.classList.add('is-on');
    if (still) { setSel(t); ready(); return; }
    stage.classList.add('is-crosshair');
    cross.style.setProperty('--cx', t.x + 'px');
    cross.style.setProperty('--cy', t.y + 'px');
    const t0 = performance.now() + 120, D = 540;
    const step = (now) => {
      const k = Math.min(1, Math.max(0, (now - t0) / D)), e = 1 - Math.pow(1 - k, 3);
      const r = { x: t.x, y: t.y, w: Math.max(1, t.w * e), h: Math.max(1, t.h * e) };
      setSel(r, false);
      cross.style.setProperty('--cx', r.x + r.w + 'px');
      cross.style.setProperty('--cy', r.y + r.h + 'px');
      if (k < 1) requestAnimationFrame(step); else { setSel(t); ready(); }
    };
    requestAnimationFrame(step);
  }
  if (document.fonts && document.fonts.status !== 'loaded') document.fonts.ready.then(start); else start();
})();
