// The note editor: a column of blocks (text, code, embeds …), saving shortly after the last change,
// undo for typing and block operations, find & replace, and the note's chrome (dates, tags, properties).

import { call, attURL } from './api.js';
import { h, toast, showError, menu, menuAt, debounce, fmtDate, prompt, modal } from './ui.js';
import * as RT from './richtext.js';
import { makeBlock } from './blocks.js';

const SAVE_DELAY = 700;

export class NoteEditor {
  constructor(app, pane, note) {
    this.app = app;
    this.pane = pane;
    this.note = note;
    this.id = note.id;
    this.blocks = [];
    this.dirty = false;
    this.undoStack = [];
    this.redoStack = [];
    this.lastSnapshot = null;
    this.saving = false;
    this.properties = note.meta.properties || [];
    this.focusedBlock = null;
    this.scheduleSave = debounce(() => this.save(), SAVE_DELAY);
    this.snapshotSoon = debounce(() => this.pushUndo(), 600);
    this.render();
  }

  get title() { return this.note.meta.title || 'Neue Notiz'; }

  render() {
    const p = this.pane;
    p.innerHTML = '';
    this.root = h('div', { class: 'editor', dataset: { note: this.id } });
    this.conflictBar = h('div', { class: 'conflict-bar hidden' });
    p.appendChild(this.conflictBar);
    this.chrome = h('div', { class: 'note-chrome' });
    this.root.appendChild(this.chrome);
    this.propsEl = h('div', { class: 'props hidden' });
    this.root.appendChild(this.propsEl);
    this.column = h('div', { class: 'blocks' });
    this.root.appendChild(this.column);
    this.linksEl = h('div', { class: 'backlinks' });
    this.root.appendChild(this.linksEl);
    p.appendChild(this.root);
    for (const b of this.note.blocks) this.addBlockView(b);
    this.renderChrome();
    this.renderProps();
    this.loadLinks();
    this.lastSnapshot = this.snapshot();
  }

  renderChrome() {
    const n = this.note;
    const c = this.chrome;
    c.innerHTML = '';
    if (n.isTemplate) c.appendChild(h('span', { class: 'chip' }, 'Vorlage'));
    c.appendChild(h('span', { title: 'Geändert' }, fmtDate(n.modified)));
    if (n.folder) c.appendChild(h('span', { class: 'chip', title: 'Ordner' }, '📁 ' + n.folder));
    for (const t of n.meta.tags || []) c.appendChild(h('span', { class: 'chip tag', onclick: () => this.app.selectTag(t) }, '#' + t));
    c.appendChild(h('span', { class: 'spacer' }));
    c.appendChild(h('button', { title: n.meta.pinned ? 'Loslösen' : 'Anheften', onclick: () => this.app.togglePin(this.id) }, n.meta.pinned ? '📌' : '📍'));
    c.appendChild(h('button', { title: 'Favorit', onclick: () => this.app.toggleFavorite(this.id) }, n.favorite ? '★' : '☆'));
    c.appendChild(h('button', { title: 'Eigenschaft hinzufügen', onclick: () => this.addProperty() }, '＋ Eigenschaft'));
    c.appendChild(h('button', { title: 'Mehr', onclick: e => this.app.noteMenu(this.id, e.currentTarget) }, '•••'));
  }

  // MARK: Properties (#121)

  renderProps() {
    const el = this.propsEl;
    el.innerHTML = '';
    el.classList.toggle('hidden', !this.properties.length);
    this.properties.forEach((p, i) => {
      const name = h('input', { class: 'pname', value: p.name, onchange: e => { p.name = e.target.value; this.markDirty(); } });
      let value;
      if (p.kind === 'checkbox') value = h('input', { type: 'checkbox', checked: p.value === 'true', onchange: e => { p.value = e.target.checked ? 'true' : 'false'; this.markDirty(); } });
      else if (p.kind === 'date') value = h('input', { type: 'date', value: p.value, onchange: e => { p.value = e.target.value; this.markDirty(); } });
      else if (p.kind === 'number') value = h('input', { type: 'number', value: p.value, onchange: e => { p.value = e.target.value; this.markDirty(); } });
      else if (p.kind === 'status') {
        value = h('select', { onchange: e => { if (e.target.value === '__new') { prompt('Neue Auswahl').then(v => { if (v) { p.options = [...(p.options || []), v]; p.value = v; this.markDirty(); this.renderProps(); } }); return; } p.value = e.target.value; this.markDirty(); } },
          h('option', { value: '' }, '—'), ...(p.options || []).map(o => h('option', { value: o, selected: o === p.value }, o)), h('option', { value: '__new' }, 'Neue Auswahl …'));
      } else value = h('input', { value: p.value, onchange: e => { p.value = e.target.value; this.markDirty(); } });
      el.append(name, value, h('button', { class: 'icon-btn small', title: 'Entfernen', onclick: () => { this.properties.splice(i, 1); this.renderProps(); this.markDirty(); } }, '×'));
    });
  }

  addProperty() {
    const kinds = [['text', 'Text'], ['number', 'Zahl'], ['date', 'Datum'], ['status', 'Auswahl'], ['checkbox', 'Ja/Nein']];
    const sel = h('select', {}, ...kinds.map(([k, l]) => h('option', { value: k }, l)));
    const name = h('input', { type: 'text', class: 'text', placeholder: 'Name, z. B. Status' });
    modal({
      title: 'Eigenschaft hinzufügen',
      body: h('div', {}, h('div', { class: 'field' }, h('label', {}, 'Name'), name), h('div', { class: 'field' }, h('label', {}, 'Art'), sel)),
      buttons: [{ label: 'Abbrechen' }, { label: 'Hinzufügen', primary: true, action: () => {
        if (!name.value.trim()) return true;
        this.properties.push({ id: crypto.randomUUID().toUpperCase(), name: name.value.trim(), kind: sel.value, value: sel.value === 'checkbox' ? 'false' : '', options: sel.value === 'status' ? ['Offen', 'In Arbeit', 'Erledigt'] : undefined });
        this.renderProps();
        this.markDirty();
      } }],
    });
    setTimeout(() => name.focus(), 30);
  }

  async loadLinks() {
    try {
      const l = await call('note_links', { id: this.id });
      const el = this.linksEl;
      el.innerHTML = '';
      const section = (title, list, extra) => {
        if (!list.length) return;
        el.appendChild(h('h4', {}, title));
        for (const n of list) el.appendChild(h('div', { class: 'bl' }, h('a', { onclick: () => this.app.openNote(n.id) }, n.title || 'Ohne Titel'), h('span', { class: 'hint' }, n.folder || ''), extra ? extra(n) : null));
      };
      section('Verlinkt von', l.backlinks);
      section('Verweist auf', l.outgoing);
      if (l.unresolved.length) {
        el.appendChild(h('h4', {}, 'Noch nicht angelegt'));
        for (const t of l.unresolved) el.appendChild(h('div', { class: 'bl' }, h('a', { onclick: () => this.app.openWiki(t, true) }, t), h('span', { class: 'hint' }, 'anlegen')));
      }
      section('Nicht verlinkte Erwähnungen', l.mentions, n => h('a', { class: 'hint', onclick: async () => { await call('link_mention', { source: n.id, target: this.id }); this.loadLinks(); this.app.refreshList(); } }, 'verlinken'));
    } catch (e) { /* not important */ }
  }

  addBlockView(data, index) {
    const view = makeBlock(this, data);
    const wrap = h('div', { class: 'block', dataset: { id: data.id, kind: data.kind } });
    if (data.kind !== 'text') {
      const handle = h('div', { class: 'block-handle' },
        h('button', { title: 'Nach oben', onclick: () => this.moveBlock(data.id, -1) }, '↑'),
        h('button', { title: 'Nach unten', onclick: () => this.moveBlock(data.id, 1) }, '↓'),
        h('button', { title: 'Löschen', onclick: () => this.removeBlock(data.id) }, '✕'));
      wrap.appendChild(handle);
    }
    wrap.appendChild(view.el);
    const entry = { id: data.id, kind: data.kind, view, wrap };
    if (index === undefined || index >= this.blocks.length) {
      this.blocks.push(entry);
      this.column.appendChild(wrap);
    } else {
      this.blocks.splice(index, 0, entry);
      this.column.insertBefore(wrap, this.blocks[index + 1].wrap);
    }
    return entry;
  }

  block(id) { return this.blocks.find(b => b.id === id); }
  indexOf(id) { return this.blocks.findIndex(b => b.id === id); }

  collect() { return this.blocks.map(b => b.view.getData()); }

  // MARK: Saving

  markDirty() {
    this.dirty = true;
    this.app.setDirty(this.id, true);
    this.scheduleSave();
    this.snapshotSoon();
  }

  async save({ force = false } = {}) {
    this.scheduleSave.cancel();
    if (!this.dirty && !force) return true;
    if (this.saving) { this.scheduleSave(); return false; }
    this.saving = true;
    this.dirty = false;
    try {
      const r = await call('note_save', { id: this.id, blocks: this.collect(), properties: this.properties, force });
      if (r.conflict) {
        this.dirty = true;
        this.showConflict();
        return false;
      }
      const titleChanged = r.meta.title !== this.note.meta.title;
      const tagsChanged = JSON.stringify(r.meta.tags) !== JSON.stringify(this.note.meta.tags);
      this.note.meta = r.meta;
      this.note.modified = r.meta.modified + 978307200;
      this.app.noteSaved(this.id, r, titleChanged || tagsChanged);
      if (tagsChanged) this.renderChrome();
      this.app.setDirty(this.id, this.dirty);
      return true;
    } catch (e) {
      this.dirty = true;
      showError(e);
      return false;
    } finally {
      this.saving = false;
    }
  }

  showConflict() {
    const bar = this.conflictBar;
    bar.innerHTML = '';
    bar.classList.remove('hidden');
    const resolve = async choice => {
      bar.classList.add('hidden');
      const r = await call('note_resolve_conflict', { id: this.id, choice });
      if (choice === 'mine') { this.dirty = true; await this.save({ force: true }); }
      if (r.reopen) this.app.reopenNote(this.id);
    };
    bar.append(h('span', {}, 'Diese Notiz wurde außerhalb von Folio geändert, während du sie bearbeitet hast.'), h('span', { class: 'spacer' }),
      h('button', { class: 'btn', onclick: () => resolve('mine') }, 'Meine behalten'),
      h('button', { class: 'btn', onclick: () => resolve('theirs') }, 'Andere übernehmen'),
      h('button', { class: 'btn primary', onclick: () => resolve('both') }, 'Beide behalten'));
  }

  // MARK: Undo

  snapshot() { return JSON.stringify({ blocks: this.collect(), props: this.properties }); }

  pushUndo() {
    const s = this.snapshot();
    if (s === this.lastSnapshot) return;
    this.undoStack.push(this.lastSnapshot);
    if (this.undoStack.length > 200) this.undoStack.shift();
    this.redoStack = [];
    this.lastSnapshot = s;
  }

  restoreSnapshot(s) {
    const st = JSON.parse(s);
    const focusId = this.focusedBlock;
    this.column.innerHTML = '';
    this.blocks.forEach(b => b.view.destroy && b.view.destroy());
    this.blocks = [];
    for (const b of st.blocks) this.addBlockView(b);
    this.properties = st.props || [];
    this.renderProps();
    this.lastSnapshot = s;
    this.dirty = true;
    this.app.setDirty(this.id, true);
    this.scheduleSave();
    const f = focusId && this.block(focusId);
    if (f && f.view.focus) f.view.focus(true);
  }

  undo() {
    this.snapshotSoon.cancel();
    this.pushUndo();
    if (!this.undoStack.length) return false;
    this.redoStack.push(this.lastSnapshot);
    this.restoreSnapshot(this.undoStack.pop());
    return true;
  }

  redo() {
    if (!this.redoStack.length) return false;
    this.undoStack.push(this.lastSnapshot);
    this.restoreSnapshot(this.redoStack.pop());
    return true;
  }

  // MARK: Block operations

  /** Inserts a new block at the cursor (a text block is split there, like in Apple Notes). */
  insertBlock(data) {
    this.pushUndo();
    let index = this.blocks.length;
    const focused = this.focusedBlock && this.block(this.focusedBlock);
    if (focused) {
      index = this.indexOf(focused.id) + 1;
      if (focused.kind === 'text') {
        const rt = focused.view;
        const rich = rt.getData().rich;
        const sel = rt.lastSelection || { start: RT.plainText(rich).length };
        const [pi, off] = RT.locate(rich, sel.start);
        const p = rich.paras[pi];
        const [l, r] = RT.splitRuns(p.runs, off);
        const before = { paras: [...rich.paras.slice(0, pi), { ...p, runs: l }] };
        const after = { paras: [{ ...p, runs: r }, ...rich.paras.slice(pi + 1)] };
        // An empty line at the cut goes away, so the new block sits where the cursor was.
        if (!RT.paraText(before.paras[before.paras.length - 1]) && before.paras.length > 1) before.paras.pop();
        if (!RT.paraText(after.paras[0]) && after.paras.length > 1) after.paras.shift();
        rt.setRich(before);
        if (RT.plainText(after).length || index >= this.blocks.length || this.blocks[index].kind !== 'text') {
          this.addBlockView({ id: crypto.randomUUID().toUpperCase(), kind: 'text', rich: after }, index);
        }
      }
    }
    const entry = this.addBlockView(data, index);
    // Every non-text block is followed by a text block, so there is always a place to type.
    const next = this.blocks[index + 1];
    if (!next || next.kind !== 'text') this.addBlockView({ id: crypto.randomUUID().toUpperCase(), kind: 'text', rich: { paras: [{ runs: [] }] } }, index + 1);
    if (index === 0) this.addBlockView({ id: crypto.randomUUID().toUpperCase(), kind: 'text', rich: { paras: [{ runs: [] }] } }, 0);
    this.markDirty();
    setTimeout(() => { entry.wrap.scrollIntoView({ block: 'nearest', behavior: 'smooth' }); entry.view.focus && entry.view.focus(); }, 30);
    return entry;
  }

  removeBlock(id) {
    const i = this.indexOf(id);
    if (i < 0) return;
    this.pushUndo();
    const b = this.blocks[i];
    b.view.destroy && b.view.destroy();
    b.wrap.remove();
    this.blocks.splice(i, 1);
    // Two text blocks next to each other become one.
    const a = this.blocks[i - 1], c = this.blocks[i];
    if (a && c && a.kind === 'text' && c.kind === 'text') {
      const ra = a.view.getData().rich, rc = c.view.getData().rich;
      const ta = RT.plainText(ra), tc = RT.plainText(rc);
      const merged = !ta ? rc : !tc ? ra : { paras: [...ra.paras, ...rc.paras] };
      a.view.setRich(merged);
      c.wrap.remove();
      this.blocks.splice(i, 1);
      a.view.focus && a.view.focus(true, ta.length);
    }
    if (!this.blocks.length) this.addBlockView({ id: crypto.randomUUID().toUpperCase(), kind: 'text', rich: { paras: [{ runs: [] }] } });
    this.markDirty();
  }

  moveBlock(id, delta) {
    const i = this.indexOf(id);
    if (i < 0) return;
    this.pushUndo();
    let j = i + delta;
    // Skip empty text blocks so the block jumps to the next real position.
    while (j > 0 && j < this.blocks.length - 1 && this.blocks[j].kind === 'text' && !RT.plainText(this.blocks[j].view.getData().rich)) j += delta;
    j = Math.max(0, Math.min(this.blocks.length - 1, j));
    if (j === i) return;
    const [b] = this.blocks.splice(i, 1);
    this.blocks.splice(j, 0, b);
    this.column.innerHTML = '';
    for (const x of this.blocks) this.column.appendChild(x.wrap);
    if (this.blocks[0].kind !== 'text') this.addBlockView({ id: crypto.randomUUID().toUpperCase(), kind: 'text', rich: { paras: [{ runs: [] }] } }, 0);
    if (this.blocks[this.blocks.length - 1].kind !== 'text') this.addBlockView({ id: crypto.randomUUID().toUpperCase(), kind: 'text', rich: { paras: [{ runs: [] }] } });
    this.markDirty();
    b.wrap.scrollIntoView({ block: 'nearest' });
  }

  /** Backspace at the start of a text block: joins it with the text before, or selects the block before. */
  joinWithPrevious(id) {
    const i = this.indexOf(id);
    const prev = this.blocks[i - 1];
    if (!prev) return;
    if (prev.kind === 'text') {
      this.pushUndo();
      const ra = prev.view.getData().rich, rb = this.blocks[i].view.getData().rich;
      const len = RT.plainText(ra).length;
      const last = ra.paras[ra.paras.length - 1];
      last.runs = [...last.runs, ...rb.paras[0].runs];
      ra.paras.push(...rb.paras.slice(1));
      prev.view.setRich(ra);
      this.blocks[i].wrap.remove();
      this.blocks.splice(i, 1);
      prev.view.focus(true, len);
      this.markDirty();
    } else {
      // An embed before: Backspace removes it only when the text block is empty, otherwise highlights it.
      const cur = this.blocks[i];
      if (!RT.plainText(cur.view.getData().rich) && i < this.blocks.length - 1) {
        this.removeBlock(cur.id);
      } else {
        prev.wrap.classList.add('selected-block');
        setTimeout(() => prev.wrap.classList.remove('selected-block'), 1200);
      }
    }
  }

  focusFirst() {
    const t = this.blocks.find(b => b.kind === 'text');
    t && t.view.focus(true, 0);
  }

  focusBlockAt(id, offset) {
    const b = this.block(id);
    b && b.view.focus && b.view.focus(true, offset);
  }

  textBlocks() { return this.blocks.filter(b => b.kind === 'text'); }

  activeText() {
    const b = this.focusedBlock && this.block(this.focusedBlock);
    return b && b.kind === 'text' ? b.view : null;
  }

  async runAll() {
    for (const b of this.blocks.filter(b => b.kind === 'code')) {
      const ok = await b.view.run();
      if (ok === false) break;
    }
  }

  async flush() { await this.save(); }

  destroy() {
    this.blocks.forEach(b => b.view.destroy && b.view.destroy());
  }

  // MARK: Find & replace (#9)

  openFind(replace) {
    if (this.findbar) { this.findbar.querySelector('input').focus(); this.findbar.querySelector('.repl').classList.toggle('hidden', !replace); return; }
    const q = h('input', { type: 'text', placeholder: 'Suchen' });
    const r = h('input', { type: 'text', placeholder: 'Ersetzen durch' });
    const cs = h('input', { type: 'checkbox', title: 'Groß/klein' });
    const ww = h('input', { type: 'checkbox', title: 'Ganzes Wort' });
    const count = h('span', { class: 'count' });
    this.findState = { hits: [], cur: -1 };
    const update = () => this.findUpdate(q.value, cs.checked, ww.checked, count);
    q.addEventListener('input', update);
    cs.addEventListener('change', update);
    ww.addEventListener('change', update);
    q.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); this.findNext(e.shiftKey ? -1 : 1, count); } if (e.key === 'Escape') this.closeFind(); });
    const repl = h('span', { class: 'repl row' + (replace ? '' : ' hidden') }, r,
      h('button', { class: 'btn', onclick: () => this.replaceCurrent(r.value, q.value, cs.checked, ww.checked, count) }, 'Ersetzen'),
      h('button', { class: 'btn', onclick: () => this.replaceAll(r.value, q.value, cs.checked, ww.checked, count) }, 'Alle'));
    this.findbar = h('div', { class: 'findbar floating' }, q, h('label', { class: 'row hint' }, cs, 'Aa'), h('label', { class: 'row hint' }, ww, 'Wort'), count,
      h('button', { class: 'icon-btn small', onclick: () => this.findNext(-1, count) }, '‹'), h('button', { class: 'icon-btn small', onclick: () => this.findNext(1, count) }, '›'), repl,
      h('span', { class: 'spacer' }), h('button', { class: 'icon-btn small', onclick: () => this.closeFind() }, '✕'));
    this.pane.insertBefore(this.findbar, this.root);
    q.focus();
  }

  closeFind() {
    if (!this.findbar) return;
    this.findbar.remove();
    this.findbar = null;
    for (const b of this.textBlocks()) b.view.clearHighlights();
  }

  findUpdate(query, caseSensitive, wholeWord, count) {
    const hits = [];
    for (const b of this.blocks) {
      if (b.kind !== 'text' && b.kind !== 'code') continue;
      const ranges = b.view.highlightFind ? b.view.highlightFind(query, caseSensitive, wholeWord) : [];
      ranges.forEach((r, i) => hits.push({ block: b, index: i, range: r }));
    }
    this.findState = { hits, cur: hits.length ? 0 : -1, query, caseSensitive, wholeWord };
    count.textContent = query ? `${hits.length ? 1 : 0} von ${hits.length}` : '';
    if (hits.length) this.showHit(0);
  }

  showHit(i) {
    const hit = this.findState.hits[i];
    if (!hit) return;
    hit.block.view.markCurrent && hit.block.view.markCurrent(hit.index);
  }

  findNext(d, count) {
    const s = this.findState;
    if (!s.hits.length) return;
    s.cur = (s.cur + d + s.hits.length) % s.hits.length;
    count.textContent = `${s.cur + 1} von ${s.hits.length}`;
    this.showHit(s.cur);
  }

  replaceCurrent(withText, q, cs, ww, count) {
    const s = this.findState;
    const hit = s.hits[s.cur];
    if (!hit) return;
    this.pushUndo();
    hit.block.view.replaceRange(hit.range, withText);
    this.markDirty();
    this.findUpdate(q, cs, ww, count);
  }

  replaceAll(withText, q, cs, ww, count) {
    this.pushUndo();
    let n = 0;
    for (const b of this.blocks) {
      if (!b.view.replaceAll) continue;
      n += b.view.replaceAll(q, withText, cs, ww);
    }
    this.markDirty();
    toast(`${n} ersetzt`);
    this.findUpdate(q, cs, ww, count);
  }
}
