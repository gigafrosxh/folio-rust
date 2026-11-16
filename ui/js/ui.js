// Small UI helpers: elements, toasts, menus, modals, prompts.

export function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === undefined || v === null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
    else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2).toLowerCase(), v);
    else if (k === 'html') el.innerHTML = v;
    else if (k === 'dataset') Object.assign(el.dataset, v);
    else if (v === true) el.setAttribute(k, '');
    else el.setAttribute(k, v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    el.appendChild(typeof c === 'string' || typeof c === 'number' ? document.createTextNode(String(c)) : c);
  }
  return el;
}

export function toast(msg, { error = false, action, actionLabel, timeout = 4200 } = {}) {
  const host = document.getElementById('toast-host');
  const t = h('div', { class: 'toast floating' + (error ? ' err' : ''), role: 'status' }, String(msg));
  if (action) t.appendChild(h('button', { onclick: () => { action(); t.remove(); } }, actionLabel || 'OK'));
  host.appendChild(t);
  setTimeout(() => t.remove(), error ? Math.max(timeout, 7000) : timeout);
  return t;
}

export function showError(e) {
  toast(typeof e === 'string' ? e : (e && e.message) || String(e), { error: true });
}

let openMenu = null;
export function closeMenu() { if (openMenu) { openMenu.remove(); openMenu = null; } }
document.addEventListener('mousedown', e => { if (openMenu && !openMenu.contains(e.target)) closeMenu(); }, true);
document.addEventListener('keydown', e => { if (e.key === 'Escape') closeMenu(); });

/** items: [{label, action, detail, disabled, checked}|'-'|{heading}] */
export function menu(x, y, items) {
  closeMenu();
  const m = h('div', { class: 'menu floating', role: 'menu' });
  for (const it of items) {
    if (!it) continue;
    if (it === '-') { m.appendChild(h('div', { class: 'sep' })); continue; }
    if (it.heading) { m.appendChild(h('div', { class: 'heading' }, it.heading)); continue; }
    m.appendChild(h('div', { class: 'item' + (it.disabled ? ' disabled' : ''), role: 'menuitem', onclick: () => { closeMenu(); it.action && it.action(); } },
      h('span', {}, (it.checked ? '✓ ' : '') + it.label), it.detail ? h('span', { class: 'd' }, it.detail) : null));
  }
  document.getElementById('menu-host').appendChild(m);
  const r = m.getBoundingClientRect();
  m.style.left = Math.min(x, window.innerWidth - r.width - 8) + 'px';
  m.style.top = Math.min(y, window.innerHeight - r.height - 8) + 'px';
  openMenu = m;
  return m;
}

export function menuAt(el, items) {
  const r = el.getBoundingClientRect();
  return menu(r.left, r.bottom + 4, items);
}

export function modal({ title, body, buttons = [], wide = false, onClose }) {
  const back = h('div', { class: 'modal-back' });
  const box = h('div', { class: 'modal' + (wide ? ' wide' : ''), role: 'dialog', 'aria-label': title || '' });
  if (title) box.appendChild(h('h2', {}, title));
  const b = h('div', { class: 'body' });
  if (body) b.appendChild(body);
  box.appendChild(b);
  const close = () => { back.remove(); document.removeEventListener('keydown', key, true); onClose && onClose(); };
  if (buttons.length) {
    const f = h('div', { class: 'foot' });
    for (const bt of buttons) {
      f.appendChild(h('button', { class: 'btn' + (bt.primary ? ' primary' : '') + (bt.danger ? ' danger' : ''), onclick: async () => { const keep = bt.action ? await bt.action() : false; if (keep !== true) close(); } }, bt.label));
    }
    box.appendChild(f);
  }
  const key = e => {
    if (e.key === 'Escape') { e.stopPropagation(); close(); }
    if (e.key === 'Enter' && !e.shiftKey && e.target.tagName !== 'TEXTAREA' && buttons.some(b => b.primary)) {
      const p = buttons.find(b => b.primary);
      if (p && e.target.closest && e.target.closest('.modal') === box) { e.preventDefault(); box.querySelector('.btn.primary').click(); }
    }
  };
  document.addEventListener('keydown', key, true);
  back.addEventListener('mousedown', e => { if (e.target === back) close(); });
  back.appendChild(box);
  document.getElementById('modal-host').appendChild(back);
  return { close, box, body: b };
}

export function prompt(title, { value = '', placeholder = '', label = '', ok = 'OK' } = {}) {
  return new Promise(resolve => {
    const input = h('input', { type: 'text', class: 'text', value, placeholder, style: { width: '100%' } });
    let done = false;
    const m = modal({
      title,
      body: h('div', {}, label ? h('div', { class: 'hint', style: { marginBottom: '6px' } }, label) : null, input),
      buttons: [{ label: 'Abbrechen' }, { label: ok, primary: true, action: () => { done = true; resolve(input.value); } }],
      onClose: () => { if (!done) resolve(null); },
    });
    setTimeout(() => { input.focus(); input.select(); }, 20);
    return m;
  });
}

export function confirmDialog(title, text, { ok = 'OK', danger = false } = {}) {
  return new Promise(resolve => {
    let done = false;
    modal({ title, body: h('div', {}, text), buttons: [{ label: 'Abbrechen' }, { label: ok, primary: !danger, danger, action: () => { done = true; resolve(true); } }], onClose: () => { if (!done) resolve(false); } });
  });
}

export function fmtDate(unix, withTime = true) {
  const d = new Date(unix * 1000);
  const opts = withTime ? { day: 'numeric', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' } : { day: 'numeric', month: 'short', year: 'numeric' };
  return d.toLocaleString('de-DE', opts);
}

export function shortDate(unix) {
  const d = new Date(unix * 1000), now = new Date();
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' });
  const y = new Date(now); y.setDate(now.getDate() - 1);
  if (d.toDateString() === y.toDateString()) return 'Gestern';
  if (now - d < 6 * 86400e3) return d.toLocaleDateString('de-DE', { weekday: 'long' });
  return d.toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit', year: '2-digit' });
}

export function fmtSize(n) {
  if (n < 1024) return n + ' Byte';
  if (n < 1024 * 1024) return (n / 1024).toFixed(0) + ' KB';
  return (n / 1024 / 1024).toFixed(1).replace('.', ',') + ' MB';
}

export function debounce(fn, ms) {
  let t;
  const d = (...a) => { clearTimeout(t); t = setTimeout(() => fn(...a), ms); };
  d.flush = (...a) => { clearTimeout(t); fn(...a); };
  d.cancel = () => clearTimeout(t);
  return d;
}

export function b64(buf) {
  const bytes = new Uint8Array(buf);
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(s);
}
