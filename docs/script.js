// screen-mark website: the first screen runs under a working copy of the app's capture overlay.
// Selection, size pill, toolbar placement and annotation shapes follow src/overlay and src/tools.
(() => {
  // The page drops to its no-JS still frame if this hasn't started within a few seconds (index.html)
  window.smReady = true;
  document.documentElement.classList.add('js');
  const stage = document.querySelector('[data-stage]');
  if (!stage) return;
  const q = (s) => stage.querySelector(s);
  const capture = q('[data-capture]'), sel = q('.sel'), pill = q('.pill'), bar = q('[data-toolbar]');
  const ink = q('.ink'), cross = q('.cross'), toast = q('.toast'), live = q('[data-live]');
  const kb = document.querySelector('[data-kb]');
  const fine = matchMedia('(hover: hover) and (pointer: fine)').matches;
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const NS = 'http://www.w3.org/2000/svg';
  const AMBER = 'var(--draw)';
  const wordmark = q('.wordmark'), now = document.querySelector('[data-now]');

  let rect = null, initial = true, tool = 'arrow', color = AMBER, shapes = [], undone = [];
  let drag = null, toastTimer = 0, armed = false, byPointer = false;

  let lastPointer = 'mouse';
  addEventListener('pointerdown', (e) => (lastPointer = e.pointerType), true);

  stage.tabIndex = -1;
  bar.inert = false;
  // A ring whenever the screen itself gets keyboard focus (the skip link, Esc), never from a click
  stage.addEventListener('focus', () => { if (!byPointer && stage.matches(':focus-visible')) stage.classList.add('kbd-focus'); });
  // Hover labels for the icon-only buttons
  for (const b of document.querySelectorAll('[data-toolbar] button, [data-kb] button')) b.title = b.getAttribute('aria-label');

  const box = () => stage.getBoundingClientRect();
  const at = (e) => { const b = box(); return { x: e.clientX - b.left, y: e.clientY - b.top }; };
  const of = (el) => { const b = box(), r = el.getBoundingClientRect(); return { x: r.left - b.left, y: r.top - b.top, w: r.width, h: r.height }; };
  const inside = (p) => rect && p.x >= rect.x && p.x <= rect.x + rect.w && p.y >= rect.y && p.y <= rect.y + rect.h;
  const px = (n) => Math.round(n * (fine ? window.devicePixelRatio || 1 : 1));
  const hit = (a, b) => a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;

  // --- Selection, size pill and toolbar, placed the way the app places them ---
  function setSel(r, withBar = true) {
    rect = r;
    stage.classList.toggle('no-sel', !r);
    stage.classList.toggle('has-bar', !!r && withBar);
    // The name stays lit while the selection is the one on the download block (styles.css)
    stage.classList.toggle('lit-name', !!r && initial);
    if (!r) { stage.classList.add('cap-out'); sel.style.width = sel.style.height = '0px'; render(); return; }
    sel.style.transform = `translate(${r.x}px, ${r.y}px)`;
    sel.style.width = r.w + 'px';
    sel.style.height = r.h + 'px';
    pill.textContent = `${px(r.w)} × ${px(r.h)}`;
    // While the download block is outside the selection it sits under the dim (styles.css lifts its text)
    if (!(initial && !withBar)) {
      const c = of(capture);
      stage.classList.toggle('cap-out', c.x < r.x - 1 || c.y < r.y - 1 || c.x + c.w > r.x + r.w + 1 || c.y + c.h > r.y + r.h + 1);
    }
    const W = stage.clientWidth, H = stage.clientHeight, pw = pill.offsetWidth, ph = pill.offsetHeight;
    // With a mouse, "the screen" is what's visible of the stage; on touch it's the whole stage, so the
    // toolbar doesn't jump when the address bar slides away
    const seen = fine ? Math.min(H, innerHeight - box().top) : H;
    let bx = 0, by = 0;
    if (withBar) {
      // Under the selection; above it when the bottom of the screen is in the way and the name isn't, as the app does.
      // Centred on the selection, or lined up with its left edge when the bar is the wider of the two.
      const bw = bar.offsetWidth, w = of(wordmark), roof = r.y - 48;
      bx = Math.max(16, Math.min(bw > r.w ? r.x : r.x + (r.w - bw) / 2, W - bw - 16));
      by = r.y + r.h + 8;
      if (fine && by + 40 > seen && roof >= 0 && !(roof < w.y + w.h + 8 && roof + 40 > w.y - 8)) by = roof;
      const op = (bar.offsetParent || stage).getBoundingClientRect(), sb = box();
      bar.style.left = bx - (op.left - sb.left) + 'px';
      bar.style.top = by - (op.top - sb.top) + 'px';
    }
    // Bottom-right outside the selection as in the app; then above it; then beside the toolbar; then just
    // inside its bottom corner if that's on screen. With nowhere left it hides rather than cover the text.
    let x = r.x + r.w - pw + 4;
    if (x + pw > W) x = r.x - 4;
    const bw = bar.offsetWidth, hits = (px_, y) => withBar && y < by + 40 && y + ph > by && px_ < bx + bw && px_ + pw > bx;
    const fits = ([px_, y]) => y >= 0 && y + ph <= seen && px_ >= 0 && px_ + pw <= W && !hits(px_, y);
    const spots = [[x, r.y + r.h + 4], [x, r.y - ph - 4]];
    if (withBar) spots.push([bx + bw + 8, by + (40 - ph) / 2]);
    const inner = r.y + r.h - 4 <= seen ? [x - 8, r.y + r.h - ph - 4] : null;
    const [pxl, y] = spots.find(fits) ?? inner ?? [0, 0];
    pill.style.visibility = spots.some(fits) || inner ? '' : 'hidden';
    pill.style.transform = `translate(${Math.max(0, pxl)}px, ${Math.max(0, y)}px)`;
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
      // A web page can't multiply into the pixels under it, so the highlight is see-through instead
      g.append(el('rect', { x, y, width: w, height: h, style: `fill:${s.color};fill-opacity:.4` }));
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
      const n = el('text', { x: s.a.x, y: s.a.y + 0.5, 'text-anchor': 'middle', 'dominant-baseline': 'central', class: 'ink-step' });
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

  // --- The corner popup, and the same words for screen readers ---
  function announce(text) {
    live.textContent = '';
    setTimeout(() => (live.textContent = text), 60);
  }
  function hideToast() { clearTimeout(toastTimer); toast.classList.remove('is-shown'); }
  // Where the popup goes: the bottom corner of what's visible of the stage; narrower beside the
  // download block and toolbar if the full width would cover them; otherwise the top. On touch it sits
  // in the viewport just below the toolbar that was tapped, or at the top of the viewport if there's no
  // room there, clear of the download block's links.
  function placeToast() {
    toast.classList.remove('is-top', 'is-fixed');
    toast.style.bottom = toast.style.width = toast.style.top = '';
    const sb = box(), hasBar = stage.classList.contains('has-bar');
    if ((!fine || lastPointer === 'touch') && hasBar) {
      toast.classList.add('is-fixed');
      const tb = bar.getBoundingClientRect(), h = toast.offsetHeight;
      toast.style.top = (tb.bottom + 8 + h <= innerHeight ? tb.bottom + 8 : 8) + 'px';
      return 'touch';
    }
    const keep = [capture, hasBar && bar].filter(Boolean).map((e) => e.getBoundingClientRect());
    const clear = () => !keep.some((r) => hit(toast.getBoundingClientRect(), r));
    toast.style.bottom = 16 + Math.max(0, sb.bottom - innerHeight) + 'px';
    if (clear()) return 'corner';
    const room = sb.right - Math.max(...keep.map((r) => r.right)) - 32;
    if (room >= 260) { toast.style.width = Math.min(380, room) + 'px'; if (clear()) return 'beside'; }
    toast.style.bottom = toast.style.width = '';
    toast.classList.add('is-top');
    return 'top';
  }
  function say(title, detail, ms = 5000, quiet = false) {
    toast.querySelector('.toast__title').textContent = title;
    toast.querySelector('.toast__detail').textContent = detail;
    toast.classList.remove('is-shown');
    void toast.offsetWidth;
    toast.classList.add('is-shown');
    const spot = placeToast();
    if (!quiet) announce(`${title}. ${detail}`);
    clearTimeout(toastTimer);
    toastTimer = setTimeout(hideToast, ms);
    return spot;
  }
  // Never let it hide what has keyboard focus
  document.addEventListener('focusin', (e) => {
    if (e.target !== stage && toast.classList.contains('is-shown') && e.target.getBoundingClientRect && hit(toast.getBoundingClientRect(), e.target.getBoundingClientRect())) hideToast();
  });
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
  const pixelsOnly = (t) => say(`${PIXELS[t]} works in the app`, 'It needs the captured pixels, which a web page doesn’t have. Shapes work here.');

  // --- Tools and colors ---
  const KEYS = { v: 'select', a: 'arrow', r: 'rect', c: 'circle', o: 'rounded', l: 'line', p: 'pencil', h: 'highlight', f: 'spotlight', t: 'text', b: 'pixelate', n: 'step', i: 'eyedropper', m: 'measure' };
  const keyOf = Object.fromEntries(Object.entries(KEYS).map(([k, v]) => [v, k]));
  // A tool the visitor picked (not the starting one) arms click-then-click drawing
  function pick(name, byUser = true) {
    tool = name;
    if (byUser) armed = true;
    bar.querySelectorAll('[data-tool]').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.tool === name)));
    document.querySelectorAll('.key.is-tool').forEach((k) => {
      const on = k.dataset.key === keyOf[name];
      k.classList.toggle('is-current', on);
      k.setAttribute('aria-pressed', String(on));
      if (on && now) now.textContent = `Current tool in the overlay: ${k.getAttribute('aria-label')}`;
    });
  }
  function press(k) {
    const key = kb && kb.querySelector(`.key[data-key="${k}"]`);
    if (key) { key.classList.add('is-down'); setTimeout(() => key.classList.remove('is-down'), 140); }
  }
  const stepNo = () => shapes.filter((x) => x.kind === 'step').length + 1 + (stage.classList.contains('is-clean') ? 0 : 1);

  // Without a mouse to drag with (keyboard, touch), a tool button draws an example of its shape
  const NAMES = { arrow: 'an arrow', rect: 'a rectangle', circle: 'a circle', rounded: 'a rounded rectangle', line: 'a line', pencil: 'a pencil stroke', highlight: 'a highlight', spotlight: 'a spotlight', step: 'a step marker', measure: 'a measurement' };
  const SELECT_NOTE = ['Select', 'In the app it moves what you drew. Here, drag outside the selection for a new one.'];
  function sample(t) {
    if (!rect) return;
    if (PIXELS[t]) { pixelsOnly(t); return; }
    if (t === 'select') { say(...SELECT_NOTE); return; }
    if (!NAMES[t]) return;
    // On touch there's no Ctrl+Z, so each tap replaces the last example instead of piling them up
    if (!fine || lastPointer === 'touch') shapes = shapes.filter((x) => !x.sample);
    const i = shapes.length % 4, fx = [0.55, 0.12, 0.6, 0.2][i], fy = [0.12, 0.2, 0.55, 0.62][i];
    const a = { x: rect.x + rect.w * fx, y: rect.y + rect.h * fy }, b = { x: a.x + rect.w * 0.25, y: a.y + rect.h * 0.28 };
    const pts = Array.from({ length: 13 }, (_, j) => ({ x: a.x + (b.x - a.x) * j / 12, y: (a.y + b.y) / 2 + Math.sin(j / 2) * rect.h * 0.08 }));
    if (t === 'arrow') [a.x, b.x] = [b.x, a.x];
    shapes.push({ kind: t, a, b, pts, color, n: stepNo(), sample: true });
    undone = [];
    render();
    say(`Drew ${NAMES[t]}`, fine ? 'Drag inside the selection to draw your own. Ctrl+Z undoes it.' : 'With a mouse you can drag inside the selection to draw your own.');
  }

  bar.addEventListener('click', (e) => {
    const b = e.target.closest('button');
    if (!b) return;
    if (b.dataset.tool) {
      const t = b.dataset.tool;
      pick(t);
      if (e.detail === 0 || !fine || lastPointer === 'touch') sample(t);
      else if (PIXELS[t]) pixelsOnly(t);
      else if (t === 'select') say(...SELECT_NOTE);
      else say(`${b.getAttribute('aria-label').replace(/ \(.\)$/, '')} picked`, 'Drag inside the selection to draw. Ctrl+Z undoes it.');
    }
    if (b.dataset.color) {
      color = b.dataset.color === 'amber' ? AMBER : b.dataset.color;
      bar.querySelectorAll('[data-color]').forEach((s) => s.setAttribute('aria-pressed', String(s === b)));
    }
    if (b.dataset.act) ACTS[b.dataset.act]();
  });
  // One tab stop per group; arrow keys move along it, Home and End jump to the ends
  function roving(group, items, start) {
    items.forEach((b, i) => (b.tabIndex = i === start ? 0 : -1));
    group.addEventListener('keydown', (e) => {
      if (!['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(e.key)) return;
      const vis = items.filter((b) => b.offsetParent);
      const i = vis.indexOf(document.activeElement);
      const n = e.key === 'Home' ? vis[0] : e.key === 'End' ? vis[vis.length - 1] : vis[(i + (e.key === 'ArrowRight' ? 1 : vis.length - 1)) % vis.length];
      items.forEach((b) => (b.tabIndex = -1));
      n.tabIndex = 0;
      n.focus();
      e.preventDefault();
    });
  }
  roving(bar, [...bar.querySelectorAll('button')], 1);

  // --- The key plate in Fourteen tools: click or tap a key to pick its tool ---
  if (kb) {
    kb.inert = false;
    const keys = [...kb.querySelectorAll('button.key')];
    roving(kb, keys, 0);
    kb.addEventListener('keydown', (e) => {
      if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return;
      const cur = document.activeElement.closest('button.key');
      if (!cur) return;
      const rows = [...kb.querySelectorAll('.kb__row')], ri = rows.indexOf(cur.parentElement) + (e.key === 'ArrowDown' ? 1 : -1);
      if (ri < 0 || ri >= rows.length) return;
      const cx = (el) => { const r = el.getBoundingClientRect(); return r.left + r.width / 2; };
      const next = [...rows[ri].querySelectorAll('button.key')].reduce((a, b) => (Math.abs(cx(b) - cx(cur)) < Math.abs(cx(a) - cx(cur)) ? b : a));
      keys.forEach((b) => (b.tabIndex = -1));
      next.tabIndex = 0;
      next.focus();
      e.preventDefault();
    });
    kb.addEventListener('click', (e) => {
      const k = e.target.closest('button.key');
      if (!k) return;
      pick(KEYS[k.dataset.key]);
      press(k.dataset.key);
    });
  }

  // --- Where the tool keys apply: only with focus in the first screen or the key plate. A click on
  // the first screen focuses it, so after the first drag the keys just work. ---
  const inZone = (zone) => zone && zone.contains(document.activeElement);
  stage.addEventListener('blur', () => stage.classList.remove('kbd-focus'));

  // --- Pointer: drag to select, click to snap to a block, draw inside the selection ---
  function clearAll() { shapes = []; undone = []; stage.classList.add('is-clean'); }
  function endDrag(e) {
    if (!drag) return;
    if (drag.mode === 'select') {
      if (drag.moved) setSel(rect);
      else if (e) {
        // A click selects the block under the pointer, like clicking a window in the app
        const win = document.elementFromPoint(e.clientX, e.clientY)?.closest('[data-window]');
        if (win) { initial = win === capture; clearAll(); const r = of(win); setSel({ x: r.x - 8, y: r.y - 8, w: r.w + 16, h: r.h + 16 }); }
      }
      stage.classList.toggle('is-crosshair', !rect);
    } else if (drag.mode === 'draw' && drag.s.kind !== 'step' && drag.s.kind !== 'pencil' && Math.hypot(drag.s.b.x - drag.s.a.x, drag.s.b.y - drag.s.a.y) < 3) {
      // A click without a drag: once a tool has been picked, the shape follows the mouse until the next
      // click (the one-pointer way to draw); before that, a stray click on the text draws nothing
      if (armed) { drag = { mode: 'place', s: drag.s }; return; }
      shapes.pop();
      render();
    }
    drag = null;
  }
  document.addEventListener('pointerdown', (e) => {
    if (!fine || e.button !== 0 || e.pointerType === 'touch' || !stage.classList.contains('is-ready')) return;
    if (drag && drag.mode === 'place') {
      if (!e.target.closest('a, button')) { drag.s.b = at(e); e.preventDefault(); }
      drag = null;
      render();
      return;
    }
    if (e.target.closest('a, button, .toast')) return;
    const p = at(e);
    if (p.y < 0 || p.y > stage.clientHeight) return;
    e.preventDefault();
    stage.classList.remove('kbd-focus');
    byPointer = true;
    stage.focus({ preventScroll: true });
    byPointer = false;
    stage.setPointerCapture(e.pointerId);
    if (inside(p) && tool !== 'select') {
      if (PIXELS[tool]) { pixelsOnly(tool); return; }
      const s = { kind: tool, a: p, b: p, pts: [p], color, n: stepNo() };
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
    if (!fine || e.pointerType === 'touch') return;
    const p = at(e);
    cross.style.setProperty('--cx', p.x + 'px');
    cross.style.setProperty('--cy', p.y + 'px');
    if (!drag) return;
    // The button came up somewhere we didn't hear about: finish there
    if (drag.mode !== 'place' && e.buttons === 0) { endDrag(e); return; }
    if (drag.mode === 'draw' || drag.mode === 'place') {
      drag.s.b = p;
      if (drag.s.kind === 'pencil') drag.s.pts.push(p);
      render();
    } else {
      if (Math.abs(p.x - drag.a.x) + Math.abs(p.y - drag.a.y) > 4 && !drag.moved) { drag.moved = true; initial = false; clearAll(); }
      if (drag.moved) setSel({ x: Math.min(drag.a.x, p.x), y: Math.min(drag.a.y, p.y), w: Math.abs(p.x - drag.a.x), h: Math.abs(p.y - drag.a.y) }, false);
    }
  });
  stage.addEventListener('pointerup', (e) => { if (drag && drag.mode !== 'place') endDrag(e); });
  stage.addEventListener('pointercancel', () => {
    if (!drag) return;
    if (drag.mode === 'draw') shapes.pop();
    if (drag.mode === 'select' && rect) setSel(rect);
    drag = null;
    render();
    stage.classList.toggle('is-crosshair', !rect);
  });

  // --- Keys: the app's tool keys, undo and redo, Enter to copy, Esc to start over ---
  function selectDownload() {
    initial = true;
    setSel(of(capture));
    stage.classList.remove('is-crosshair');
    say('Download block selected', 'Tab to the toolbar to pick a tool, or press Enter to copy.');
  }
  addEventListener('keydown', (e) => {
    if (!stage.classList.contains('is-ready')) return;
    const k = e.key.toLowerCase(), onStage = inZone(stage);
    if (!onStage && !(KEYS[k] && inZone(kb))) return;
    if (e.ctrlKey && k === 'z') {
      if (!shapes.length && !undone.length) return;
      if (e.shiftKey) { if (undone.length) shapes.push(undone.pop()); } else if (shapes.length) undone.push(shapes.pop());
      drag = null;
      render();
      e.preventDefault();
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey || e.repeat) return;
    const free = document.activeElement === stage;
    if (k === 'escape' && drag && drag.mode === 'place') {
      shapes.pop();
      drag = null;
      render();
    } else if (k === 'escape' && rect && (free || bar.contains(document.activeElement))) {
      // Only from the screen itself or its toolbar; Esc on a link leaves the link alone
      initial = false;
      clearAll();
      // From the toolbar (about to hide), focus comes back to the screen, where Enter reselects
      if (!free) { stage.focus({ preventScroll: true }); stage.classList.add('kbd-focus'); }
      setSel(null);
      stage.classList.toggle('is-crosshair', fine);
      say('Overlay cleared', fine ? 'Drag anywhere on this screen for a new selection, click a block to select it, or press Enter to select the download block.' : 'Press Enter to select the download block again.');
    } else if (k === 'enter' && onStage && free) {
      if (rect) ACTS.copy(); else selectDownload();
      e.preventDefault();
    } else if (KEYS[k] && !document.activeElement.closest('a')) {
      pick(KEYS[k]);
      press(k);
      const key = kb && kb.querySelector(`[data-key="${k}"]`);
      if (key) announce(`Current tool: ${key.getAttribute('aria-label')}`);
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
    const done = (ok) => {
      b.textContent = ok ? 'Copied' : 'Press Ctrl+C';
      announce(ok ? 'Copied the cargo command' : 'Couldn’t copy. The command is selected; press Ctrl+C.');
      setTimeout(() => (b.textContent = 'Copy'), 2500);
    };
    // Without clipboard access, select the command so it can be copied by hand
    const selectIt = () => { const r = document.createRange(); r.selectNodeContents(b.previousElementSibling); getSelection().removeAllRanges(); getSelection().addRange(r); done(false); };
    if (navigator.clipboard) navigator.clipboard.writeText(b.dataset.copy).then(() => done(true), selectIt);
    else selectIt();
  }));

  // --- Keep the first selection on the download block when the layout changes (fonts, resizes) ---
  let rz = 0;
  const follow = () => {
    cancelAnimationFrame(rz);
    rz = requestAnimationFrame(() => { if (drag) return; if (initial && rect) setSel(of(capture)); else if (rect) setSel(rect); });
  };
  addEventListener('resize', follow);
  if ('ResizeObserver' in window) new ResizeObserver(() => { if (initial && rect) follow(); }).observe(capture);

  // --- Entrance: dim, crosshair to the corner, drag the selection open, then the toolbar and notes ---
  function ready() {
    stage.classList.remove('is-crosshair');
    stage.classList.add('is-ready');
    if (fine) {
      stage.classList.add('is-live');
      const spot = say('This page is under the overlay', 'Drag anywhere to select, then draw inside it. A, R, C, L, P, N and M pick tools; Esc starts over.', 12000, true);
      if (spot === 'top') { hideToast(); return; }
      // It's a hint, not a dialog: the first thing you do puts it away
      const away = (e) => {
        if (e.type === 'keydown' && (e.key === 'Shift' || e.key === 'Control')) return;
        hideToast();
        removeEventListener('pointerdown', away, true);
        removeEventListener('keydown', away, true);
      };
      addEventListener('pointerdown', away, true);
      addEventListener('keydown', away, true);
    }
  }
  let started = false;
  function start() {
    if (started) return;
    started = true;
    pick('arrow', false);
    const t = of(capture);
    stage.classList.add('is-on');
    if (still) { setSel(t); ready(); return; }
    stage.classList.add('is-crosshair');
    cross.style.setProperty('--cx', t.x + 'px');
    cross.style.setProperty('--cy', t.y + 'px');
    const t0 = performance.now(), D = 540;
    const step = (now) => {
      const k = Math.min(1, Math.max(0, (now - t0) / D)), e = 1 - Math.pow(1 - k, 3);
      const r = { x: t.x, y: t.y, w: Math.max(1, t.w * e), h: Math.max(1, t.h * e) };
      setSel(r, false);
      cross.style.setProperty('--cx', r.x + r.w + 'px');
      cross.style.setProperty('--cy', r.y + r.h + 'px');
      if (k < 1) requestAnimationFrame(step); else { setSel(of(capture)); ready(); }
    };
    requestAnimationFrame(step);
  }
  // Start once the fonts are in, or soon anyway on a slow connection; the observer above re-measures later
  if (document.fonts && document.fonts.status !== 'loaded') { document.fonts.ready.then(start); setTimeout(start, 900); } else start();
})();
