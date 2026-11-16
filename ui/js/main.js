// Folio's main window: sidebar, note list, tabs, split view, editor, menus and shortcuts.

import { call, inApp, attURL, onNative, native, openDialog, saveDialog, pollJob } from './api.js';
import { h, toast, showError, menu, menuAt, modal, prompt, confirmDialog, fmtDate, shortDate, fmtSize, debounce, b64, closeMenu } from './ui.js';
import * as RT from './richtext.js';
import { setLanguages, languages } from './highlight.js';
import { NoteEditor } from './editor.js';
import { drawingEditor } from './blocks.js';
import * as Panels from './panels.js';

class App {
  constructor() {
    this.selection = { type: 'all' };
    this.query = '';
    this.filter = {};
    this.notes = [];
    this.tabs = [];
    this.active = null;
    this.split = null;
    this.editors = {};
    this.dirty = new Set();
    this.attVersions = {};
    this.expanded = new Set(JSON.parse(localStorage.getItem('folio.expanded') || '[]'));
    this.lastFocusedPane = 'left';
  }

  async start() {
    const init = await call('init');
    this.version = init.version;
    this.settings = init.settings;
    this.contentTypes = init.contentTypes;
    setLanguages(init.languages);
    this.applySettings();
    this.bindStatic();
    await this.refresh();
    const params = new URLSearchParams(location.search);
    if (params.get('single')) {
      // A note in its own window (⌥⌘O): only the editor.
      this.single = true;
      document.body.classList.add('focus-mode');
      await this.openNote(params.get('note'));
      setInterval(() => this.checkExternal(), 2000);
      window.__folio = this;
      document.body.dataset.ready = '1';
      return;
    }
    const saved = JSON.parse(localStorage.getItem('folio.tabs') || 'null');
    if (saved && saved.tabs) {
      this.tabs = saved.tabs.filter(id => this.knows(id));
      if (this.tabs.length) await this.openNote(this.knows(saved.active) ? saved.active : this.tabs[0]);
    }
    if (!this.active && this.notes.length) await this.openNote(this.notes[0].id);
    this.renderTabs();
    setInterval(() => this.checkExternal(), 2000);
    setTimeout(() => call('backup_due').catch(() => {}), 15000);
    window.addEventListener('focus', () => this.refresh());
    window.__folio = this;
    document.body.dataset.ready = '1';
    if (init.selftest) import('./selftest.js').then(m => m.run(this)).catch(e => call('selftest_report', { lines: ['FAIL Selbsttest: ' + e], failed: 1 }));
  }

  knows(id) { return id && (this.allIds || new Set()).has(id); }

  applySettings() {
    const s = this.settings;
    const root = document.documentElement;
    root.dataset.theme = s.appearance === 'system' ? '' : s.appearance;
    if (!root.dataset.theme) delete root.dataset.theme;
    root.dataset.floating = s.floatingStyle;
    root.style.setProperty('--accent', s.accentHex);
    root.style.setProperty('--font-size', s.editorFontSize + 'px');
    root.style.setProperty('--line-spacing', s.lineSpacing + 'px');
    root.style.setProperty('--editor-width', s.editorWidth + 'px');
    root.style.setProperty('--code-size', s.codeFontSize + 'px');
    root.style.setProperty('--radius', s.cornerRadius + 'px');
    root.style.setProperty('--code-radius', '12px');
    if (s.editorFont && s.editorFont !== 'System') root.style.setProperty('--body-font', `"${s.editorFont}", -apple-system, sans-serif`);
    else root.style.removeProperty('--body-font');
    if (s.codeFont) root.style.setProperty('--mono', `"${s.codeFont}", ui-monospace, Menlo, monospace`);
    RT.setBaseSize(s.editorFontSize);
    RT.setWikiResolver(t => this.titleKeys && this.titleKeys.has(t.trim().toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '')));
    native('set_theme', { theme: s.appearance }).catch(() => {});
  }

  // MARK: Library and list

  async refresh() {
    try {
      this.library = await call('library');
      if (this.library.error) toast(this.library.error, { error: true });
      this.renderSidebar();
      await this.refreshList();
    } catch (e) { showError(e); }
  }

  async refreshList() {
    const r = await call('notes_list', { selection: this.selection, query: this.query, filter: this.filter });
    this.notes = r.notes;
    if (this.selection.type === 'all' && !this.query && !Object.keys(this.filter).length) {
      this.allIds = new Set(r.notes.map(n => n.id));
      this.titleKeys = new Set(r.notes.map(n => (n.title || '').trim().toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '')));
    } else if (!this.allIds) {
      const all = await call('notes_list', { selection: { type: 'all' } });
      this.allIds = new Set(all.notes.map(n => n.id));
      this.titleKeys = new Set(all.notes.map(n => (n.title || '').trim().toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '')));
    }
    const sum = document.getElementById('search-summary');
    sum.classList.toggle('hidden', !r.searchSummary);
    sum.innerHTML = '';
    if (r.searchSummary) sum.append(h('span', {}, r.searchSummary), h('button', { onclick: () => this.saveSmartView() }, 'Als intelligente Ansicht sichern'), h('button', { onclick: () => { this.filter = {}; this.setQuery(''); } }, '✕'));
    this.renderList();
  }

  async refreshAllIds() {
    const all = await call('notes_list', { selection: { type: 'all' } });
    this.allIds = new Set(all.notes.map(n => n.id));
    this.titleKeys = new Set(all.notes.map(n => (n.title || '').trim().toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '')));
  }

  selectionLabel() {
    const s = this.selection;
    return { all: 'Alle Notizen', pinned: 'Angeheftet', favorites: 'Favoriten', templates: 'Vorlagen', trash: 'Papierkorb', damaged: 'Beschädigte Notizen', graph: 'Graph' }[s.type]
      || (s.type === 'folder' ? (s.value.split('/').pop() || 'Bibliothek') : s.type === 'tag' ? '#' + s.value : s.type === 'smart' ? ((this.library.smartViews.find(v => v.id === s.value) || {}).name || 'Ansicht') : '');
  }

  renderSidebar() {
    const L = this.library;
    const nav = document.getElementById('nav');
    nav.innerHTML = '';
    const item = (sel, icon, label, count, extra = {}) => {
      const active = this.selection.type === sel.type && (this.selection.value || '') === (sel.value || '');
      const el = h('div', { class: 'nav-item' + (active ? ' active' : ''), role: 'button', tabindex: '-1', dataset: { sel: JSON.stringify(sel) }, onclick: () => this.select(sel) },
        extra.twisty !== undefined ? h('span', { class: 'twisty', onclick: e => { e.stopPropagation(); extra.onTwisty(); } }, extra.twisty) : null,
        h('span', { class: 'icon' }, icon), h('span', { class: 'label' }, label), count !== undefined && count !== null ? h('span', { class: 'count' }, count) : null);
      if (extra.onContext) el.addEventListener('contextmenu', e => { e.preventDefault(); extra.onContext(e); });
      if (extra.folder !== undefined) this.makeDropTarget(el, extra.folder);
      if (extra.style) Object.assign(el.style, extra.style);
      return el;
    };
    const sec = (title, action) => {
      const s = h('div', { class: 'nav-section' }, h('div', { class: 'nav-heading' }, title, action || null));
      nav.appendChild(s);
      return s;
    };
    const top = sec('Bibliothek');
    top.append(item({ type: 'all' }, '◧', 'Alle Notizen', L.total), item({ type: 'pinned' }, '📌', 'Angeheftet', L.pinned), item({ type: 'favorites' }, '★', 'Favoriten', L.favorites),
      h('div', { class: 'nav-item', onclick: () => this.dailyNote() }, h('span', { class: 'icon' }, '☀︎'), h('span', { class: 'label' }, 'Tagesnotiz')),
      item({ type: 'graph' }, '◎', 'Graph'));
    const folders = sec('Ordner', h('button', { title: 'Neuer Ordner', onclick: () => this.newFolder('') }, '＋'));
    const walk = (list, depth, parent) => {
      for (const f of list) {
        const open = this.expanded.has(f.path);
        const el = item({ type: 'folder', value: f.path }, '📁', f.name, f.count, {
          folder: f.path, style: { paddingLeft: (8 + depth * 14) + 'px' },
          twisty: f.children.length ? (open ? '▾' : '▸') : '',
          onTwisty: () => { open ? this.expanded.delete(f.path) : this.expanded.add(f.path); localStorage.setItem('folio.expanded', JSON.stringify([...this.expanded])); this.renderSidebar(); },
          onContext: e => this.folderMenu(e, f, parent, list),
        });
        el.draggable = true;
        el.addEventListener('dragstart', e => e.dataTransfer.setData('folio/folder', JSON.stringify({ name: f.name, parent, visible: list.map(x => x.name) })));
        folders.appendChild(el);
        if (open) walk(f.children, depth + 1, f.path);
      }
    };
    walk(L.folders, 0, '');
    if (L.smartViews.length) {
      const sv = sec('Intelligente Ansichten');
      for (const v of L.smartViews) sv.appendChild(item({ type: 'smart', value: v.id }, '⚙︎', v.name, v.count, { onContext: e => this.smartViewMenu(e, v) }));
    }
    if (L.tags.length) {
      const tg = sec('Tags');
      for (const t of L.tags) tg.appendChild(item({ type: 'tag', value: t.tag }, '#', t.tag, t.count, { onContext: e => this.tagMenu(e, t.tag) }));
    }
    const more = sec('Weiteres');
    more.append(item({ type: 'templates' }, '▤', 'Vorlagen', L.templates || null), item({ type: 'trash' }, '🗑', 'Papierkorb', L.trash || null));
    if (L.damaged) more.appendChild(item({ type: 'damaged' }, '⚠︎', 'Beschädigte Notizen', L.damaged));
    document.getElementById('list-title').textContent = this.selectionLabel();
  }

  makeDropTarget(el, folder) {
    el.addEventListener('dragover', e => { if (e.dataTransfer.types.includes('folio/note') || e.dataTransfer.types.includes('folio/folder')) { e.preventDefault(); el.classList.add('drop'); } });
    el.addEventListener('dragleave', () => el.classList.remove('drop'));
    el.addEventListener('drop', async e => {
      el.classList.remove('drop');
      const id = e.dataTransfer.getData('folio/note');
      if (id) { e.preventDefault(); await this.moveNote(id, folder); return; }
      const f = e.dataTransfer.getData('folio/folder');
      if (f) {
        e.preventDefault();
        const src = JSON.parse(f);
        const target = folder.split('/').pop();
        const parent = folder.includes('/') ? folder.slice(0, folder.lastIndexOf('/')) : '';
        if (parent === src.parent) { await call('folder_move_order', { parent, name: src.name, before: target, visible: src.visible }); this.refresh(); }
      }
    });
  }

  async select(sel) {
    if (sel.type === 'graph') { this.selection = sel; this.renderSidebar(); this.showGraph(); return; }
    if (sel.type === 'trash') { this.selection = sel; this.renderSidebar(); Panels.trashView(this); return; }
    if (sel.type === 'damaged') { this.selection = sel; this.renderSidebar(); Panels.damagedView(this); return; }
    this.selection = sel;
    this.renderSidebar();
    if (this.graphShown || this.specialShown) { this.graphShown = false; this.specialShown = false; this.showActive(); }
    await this.refreshList();
  }

  selectTag(t) { this.select({ type: 'tag', value: t }); }

  renderList() {
    const list = document.getElementById('note-list');
    list.innerHTML = '';
    if (!this.notes.length) {
      list.appendChild(h('div', { class: 'empty' }, this.query ? 'Keine Treffer.' : this.selection.type === 'templates' ? 'Noch keine Vorlagen.' : 'Keine Notizen.'));
      return;
    }
    let group = null;
    const frag = document.createDocumentFragment();
    const manual = this.library.sortMode === 'manual';
    for (const n of this.notes) {
      if (n.group && n.group !== group) { group = n.group; frag.appendChild(h('div', { class: 'group-title' }, group)); }
      const row = h('div', { class: 'note-row' + (n.id === this.active ? ' active' : ''), dataset: { id: n.id }, draggable: 'true' },
        h('div', { class: 't' }, n.title || 'Ohne Titel'),
        h('div', { class: 'm' }, h('span', {}, shortDate(this.library.sortMode === 'created' ? n.created : n.modified)), h('span', { class: 'p' }, n.preview || (n.folder && this.selection.type !== 'folder' ? n.folder : ''))),
        n.snippet ? h('div', { class: 'snip' }, n.snippet) : null,
        h('div', { class: 'badges' }, (n.pinned ? '📌' : '') + (n.favorite ? '★' : '')));
      row.addEventListener('click', e => this.openNote(n.id, { newTab: e.metaKey }));
      row.addEventListener('contextmenu', e => { e.preventDefault(); this.noteMenu(n.id, null, e); });
      row.addEventListener('dragstart', e => { e.dataTransfer.setData('folio/note', n.id); e.dataTransfer.setData('text/plain', `[[${n.title}]]`); });
      row.addEventListener('dragover', e => { if (e.dataTransfer.types.includes('folio/note')) { e.preventDefault(); row.classList.add('drag-over'); } });
      row.addEventListener('dragleave', () => row.classList.remove('drag-over'));
      row.addEventListener('drop', async e => {
        row.classList.remove('drag-over');
        const id = e.dataTransfer.getData('folio/note');
        if (!id || id === n.id) return;
        e.preventDefault();
        await call('note_move_order', { selection: this.selection, id, before: n.id, visible: this.notes.map(x => x.id) });
        if (!manual) toast('Sortierung: Eigene Reihenfolge');
        await this.refresh();
      });
      frag.appendChild(row);
    }
    list.appendChild(frag);
  }

  setQuery(q) {
    this.query = q;
    document.getElementById('search').value = q;
    this.refreshList();
  }

  // MARK: Opening notes, tabs, split view

  async openNote(id, { newTab = false, pane } = {}) {
    if (!id) return;
    if (pane === 'right') return this.openSplit(id);
    if (this.active && this.editors[this.active] && this.active !== id) await this.editors[this.active].flush();
    if (!this.tabs.includes(id)) {
      if (newTab || !this.active) this.tabs.push(id);
      else { const i = this.tabs.indexOf(this.active); if (i >= 0) this.tabs[i] = id; else this.tabs.push(id); }
    }
    this.active = id;
    this.graphShown = false;
    this.specialShown = false;
    await this.showActive();
    this.renderTabs();
    document.querySelectorAll('.note-row').forEach(r => r.classList.toggle('active', r.dataset.id === id));
    this.persistTabs();
    this.releaseUnused();
  }

  async editorFor(id, pane) {
    const ed = this.editors[id];
    if (ed && ed.pane === pane) return ed;
    if (ed) { await ed.flush(); ed.destroy(); }
    const note = await call('note_open', { id });
    const e = new NoteEditor(this, pane, note);
    this.editors[id] = e;
    return e;
  }

  async showActive() {
    const pane = document.getElementById('pane-left');
    if (!this.active) { pane.innerHTML = ''; pane.appendChild(h('div', { class: 'empty' }, 'Keine Notiz ausgewählt.')); this.updateFormatBar(); return; }
    try {
      const ed = await this.editorFor(this.active, pane);
      if (pane.firstChild !== ed.root && !pane.contains(ed.root)) ed.render();
    } catch (e) {
      pane.innerHTML = '';
      pane.appendChild(h('div', { class: 'damaged-view' }, h('h2', {}, 'Diese Notiz lässt sich nicht öffnen'), h('p', {}, String(e))));
    }
    this.updateFormatBar();
  }

  async openSplit(id) {
    const pane = document.getElementById('pane-right');
    if (!id) { this.split = null; pane.classList.add('hidden'); pane.innerHTML = ''; this.releaseUnused(); return; }
    if (id === this.active) { toast('Diese Notiz ist schon links geöffnet.'); return; }
    this.split = id;
    pane.classList.remove('hidden');
    await this.editorFor(id, pane);
  }

  releaseUnused() {
    for (const [id, ed] of Object.entries(this.editors)) {
      if (id === this.active || id === this.split) continue;
      if (!this.tabs.includes(id)) { ed.flush().then(() => { ed.destroy(); delete this.editors[id]; call('note_close', { id }); }); }
    }
  }

  renderTabs() {
    const bar = document.getElementById('tabbar');
    bar.innerHTML = '';
    for (const id of this.tabs) {
      const n = this.noteSummary(id);
      const t = h('div', { class: 'tab' + (id === this.active ? ' active' : ''), onclick: () => this.openNote(id) },
        h('span', { class: 'tt' }, (this.dirty.has(id) ? '• ' : '') + ((n && n.title) || (this.editors[id] && this.editors[id].title) || 'Notiz')),
        h('button', { class: 'x', title: 'Tab schließen (⌘W)', onclick: e => { e.stopPropagation(); this.closeTab(id); } }, '×'));
      t.addEventListener('auxclick', e => { if (e.button === 1) this.closeTab(id); });
      bar.appendChild(t);
    }
    bar.appendChild(h('button', { class: 'tab-plus', title: 'Neuer Tab (⌘T)', onclick: () => this.newNote({ newTab: true }) }, '+'));
  }

  noteSummary(id) { return this.notes.find(n => n.id === id) || (this.summaries && this.summaries[id]); }

  async closeTab(id) {
    id = id || this.active;
    const i = this.tabs.indexOf(id);
    if (i < 0) return;
    if (this.editors[id]) await this.editors[id].flush();
    this.tabs.splice(i, 1);
    if (this.active === id) {
      this.active = this.tabs[Math.min(i, this.tabs.length - 1)] || null;
      await this.showActive();
    }
    this.renderTabs();
    this.persistTabs();
    this.releaseUnused();
  }

  selectTab(d) {
    if (!this.tabs.length) return;
    const i = (this.tabs.indexOf(this.active) + d + this.tabs.length) % this.tabs.length;
    this.openNote(this.tabs[i]);
  }

  persistTabs() { if (this.single) return; localStorage.setItem('folio.tabs', JSON.stringify({ tabs: this.tabs, active: this.active })); }

  async reopenNote(id) {
    const ed = this.editors[id];
    if (!ed) return;
    const pane = ed.pane;
    ed.destroy();
    delete this.editors[id];
    await call('note_close', { id });
    await this.editorFor(id, pane);
  }

  currentEditor() {
    const left = this.active && this.editors[this.active];
    const right = this.split && this.editors[this.split];
    if (right && this.lastFocusedPane === 'right') return right;
    return left || right || null;
  }

  setDirty(id, d) {
    const had = this.dirty.has(id);
    d ? this.dirty.add(id) : this.dirty.delete(id);
    if (had !== d) this.renderTabs();
  }

  noteSaved(id, r, listChanged) {
    this.summaries = this.summaries || {};
    if (r.summary) this.summaries[id] = r.summary;
    const i = this.notes.findIndex(n => n.id === id);
    if (i >= 0 && r.summary) Object.assign(this.notes[i], r.summary);
    this.refreshListSoon();
    if (listChanged) { this.renderTabs(); this.refreshSidebarSoon(); this.refreshAllIds(); }
  }

  refreshListSoon = debounce(() => this.refreshList(), 400);
  refreshSidebarSoon = debounce(async () => { this.library = await call('library'); this.renderSidebar(); }, 800);

  async checkExternal() {
    try {
      const r = await call('note_check_external');
      for (const id of r.changed) {
        const ed = this.editors[id];
        if (!ed) continue;
        if (ed.dirty) ed.showConflict();
        else await this.reopenNote(id);
      }
    } catch { /* ignore */ }
  }

  // MARK: Note actions

  async newNote({ newTab = false, template, title, text } = {}) {
    const folder = this.selection.type === 'folder' ? this.selection.value : (this.active && this.noteSummary(this.active) ? this.noteSummary(this.active).folder : undefined);
    const tag = this.selection.type === 'tag' ? this.selection.value : undefined;
    try {
      const r = await call('note_create', { folder, tag, template, title, text });
      if (['pinned', 'favorites', 'smart', 'trash', 'damaged', 'templates', 'graph'].includes(this.selection.type)) this.selection = { type: 'all' };
      if (this.query) { this.query = ''; document.getElementById('search').value = ''; this.filter = {}; }
      await this.refresh();
      await this.openNote(r.id, { newTab });
      const ed = this.editors[r.id];
      if (ed) {
        if (r.cursor) ed.focusBlockAt(r.cursor.block, r.cursor.offset); else ed.focusFirst();
      }
      return r.id;
    } catch (e) { showError(e); }
  }

  async togglePin(id) {
    await this.editors[id]?.flush();
    await call('note_pin', { id });
    await this.reopenIfOpen(id);
    this.refresh();
  }

  async toggleFavorite(id) {
    const f = await call('note_favorite', { id });
    const ed = this.editors[id];
    if (ed) { ed.note.favorite = f; ed.renderChrome(); }
    this.refresh();
  }

  async reopenIfOpen(id) { if (this.editors[id]) await this.reopenNote(id); }

  async deleteNote(id) {
    id = id || this.active;
    if (!id) return;
    const n = this.noteSummary(id);
    await this.editors[id]?.flush();
    try {
      await call('note_delete', { id });
      toast(`„${(n && n.title) || 'Notiz'}“ liegt im Papierkorb.`, { action: () => this.select({ type: 'trash' }), actionLabel: 'Anzeigen' });
      const idx = this.notes.findIndex(x => x.id === id);
      if (this.editors[id]) { this.editors[id].destroy(); delete this.editors[id]; }
      this.tabs = this.tabs.filter(t => t !== id);
      if (this.split === id) this.openSplit(null);
      if (this.active === id) {
        this.active = null;
        const next = this.notes[idx + 1] || this.notes[idx - 1];
        await this.refresh();
        if (next && next.id !== id) await this.openNote(next.id); else { this.renderTabs(); await this.showActive(); }
      } else { this.renderTabs(); await this.refresh(); }
    } catch (e) { showError(e); }
  }

  async duplicate(id) {
    id = id || this.active;
    await this.editors[id]?.flush();
    const r = await call('note_duplicate', { id });
    await this.refresh();
    this.openNote(r.id);
  }

  async moveNote(id, folder) {
    await this.editors[id]?.flush();
    try { await call('note_move', { id, folder }); toast(`Verschoben nach „${folder || 'Bibliothek'}“`); await this.refresh(); this.reopenIfOpen(id); } catch (e) { showError(e); }
  }

  async moveNoteDialog(id) {
    const folders = [];
    const walk = (l) => { for (const f of l) { folders.push(f.path); walk(f.children); } };
    walk(this.library.folders);
    menu(innerWidth / 2 - 120, 120, [{ heading: 'Verschieben nach' }, { label: 'Bibliothek (oberste Ebene)', action: () => this.moveNote(id, '') }, ...folders.map(f => ({ label: f, action: () => this.moveNote(id, f) }))]);
  }

  noteMenu(id, anchor, ev) {
    const n = this.noteSummary(id) || {};
    const items = [
      { label: 'In neuem Tab öffnen', action: () => this.openNote(id, { newTab: true }) },
      { label: 'Rechts daneben öffnen', detail: '⌥⌘\\', action: () => this.openSplit(id) },
      inApp ? { label: 'In neuem Fenster öffnen', detail: '⌥⌘O', action: () => native('open_note_window', { id }) } : null,
      '-',
      { label: n.pinned ? 'Loslösen' : 'Anheften', action: () => this.togglePin(id) },
      { label: n.favorite ? 'Aus Favoriten entfernen' : 'Zu Favoriten', action: () => this.toggleFavorite(id) },
      { label: 'Duplizieren', detail: '⌘D', action: () => this.duplicate(id) },
      { label: 'Verschieben …', action: () => this.moveNoteDialog(id) },
      { label: 'Als Vorlage sichern', action: async () => { await this.editors[id]?.flush(); await call('template_from_note', { id }); toast('Als Vorlage gesichert'); this.refresh(); } },
      '-',
      { label: 'Versionsverlauf …', detail: '⌥⌘Y', action: () => Panels.history(this, id) },
      { label: 'Informationen', action: () => Panels.info(this, id) },
      { label: 'Link kopieren', action: () => { navigator.clipboard.writeText(`folio://note/${id}`); toast('Link kopiert'); } },
      { label: 'Im Finder zeigen', action: async () => { const info = await call('note_info', { id }); call('reveal', { path: info.path }); } },
      { label: 'Als Markdown exportieren …', action: () => this.exportMarkdown(id) },
      '-',
      { label: 'In den Papierkorb', action: () => this.deleteNote(id) },
    ];
    if (anchor) menuAt(anchor, items); else menu(ev.clientX, ev.clientY, items);
  }

  folderMenu(e, f, parent, siblings) {
    menu(e.clientX, e.clientY, [
      { label: 'Neue Notiz hier', action: async () => { this.selection = { type: 'folder', value: f.path }; await this.newNote(); } },
      { label: 'Neuer Unterordner …', action: () => this.newFolder(f.path) },
      { label: 'Umbenennen …', action: async () => { const n = await prompt('Ordner umbenennen', { value: f.name }); if (n && n !== f.name) { try { const np = await call('folder_rename', { path: f.path, name: n }); if (this.selection.value === f.path) this.selection = { type: 'folder', value: np }; this.refresh(); } catch (er) { showError(er); } } } },
      { label: 'Nach oben', action: async () => { const v = siblings.map(x => x.name); const i = v.indexOf(f.name); if (i > 0) { await call('folder_move_order', { parent, name: f.name, before: v[i - 1], visible: v }); this.refresh(); } } },
      { label: 'Alphabetisch sortieren', action: async () => { await call('folder_sort_alpha', { parent }); this.refresh(); } },
      { label: 'Als Markdown exportieren …', action: () => toast('Ordner-Export: bitte Notizen einzeln exportieren (folgt).') },
      '-',
      { label: 'In den Papierkorb', action: async () => { if (await confirmDialog('Ordner löschen?', `„${f.name}“ und alle Notizen darin kommen in den Papierkorb.`, { ok: 'In den Papierkorb', danger: true })) { try { await call('folder_delete', { path: f.path }); this.selection = { type: 'all' }; this.refresh(); } catch (er) { showError(er); } } } },
    ]);
  }

  tagMenu(e, tag) {
    menu(e.clientX, e.clientY, [
      { label: 'Umbenennen …', action: async () => { const n = await prompt(`#${tag} umbenennen`, { value: tag, label: 'In allen Notizen; verschachtelte Tags ziehen mit.' }); if (n && n !== tag) { await this.flushAll(); const c = await call('tag_edit', { tag, to: n }); toast(`${c} Notizen geändert`); this.reloadAllEditors(); this.refresh(); } } },
      { label: 'Aus allen Notizen entfernen', action: async () => { if (await confirmDialog(`#${tag} entfernen?`, 'Der Tag wird in allen Notizen gelöscht.', { ok: 'Entfernen', danger: true })) { await this.flushAll(); const c = await call('tag_edit', { tag }); toast(`${c} Notizen geändert`); this.reloadAllEditors(); this.refresh(); } } },
    ]);
  }

  smartViewMenu(e, v) {
    menu(e.clientX, e.clientY, [
      { label: 'Umbenennen …', action: async () => { const n = await prompt('Ansicht umbenennen', { value: v.name }); if (n) { await call('smart_view_rename', { id: v.id, name: n }); this.refresh(); } } },
      { label: 'Mit aktueller Suche ersetzen', action: async () => { await call('smart_view_save', { id: v.id, name: v.name, query: this.query, filter: this.filter }); this.refresh(); } },
      { label: 'Löschen', action: async () => { await call('smart_view_delete', { id: v.id }); if (this.selection.value === v.id) this.selection = { type: 'all' }; this.refresh(); } },
    ]);
  }

  async saveSmartView() {
    const name = await prompt('Intelligente Ansicht', { placeholder: 'Name', label: 'Die aktuelle Suche mit ihren Filtern erscheint in der Seitenleiste.' });
    if (!name) return;
    const id = await call('smart_view_save', { name, query: this.query, filter: this.filter });
    this.query = '';
    this.filter = {};
    document.getElementById('search').value = '';
    await this.refresh();
    this.select({ type: 'smart', value: id });
  }

  async newFolder(parent) {
    const name = await prompt('Neuer Ordner', { placeholder: 'Name' });
    if (!name) return;
    try {
      const rel = await call('folder_create', { name, parent: parent || '' });
      if (parent) this.expanded.add(parent);
      await this.refresh();
      this.select({ type: 'folder', value: rel });
    } catch (e) { showError(e); }
  }

  async flushAll() { for (const ed of Object.values(this.editors)) await ed.flush(); }

  async reloadAllEditors() { for (const id of Object.keys(this.editors)) await this.reopenNote(id); }

  async dailyNote() {
    const r = await call('daily_note', {});
    await this.refresh();
    this.openNote(r.id);
  }

  async openWiki(title, create) {
    const r = await call('note_by_title', { title, create, folder: this.selection.type === 'folder' ? this.selection.value : undefined });
    if (!r.id) return toast(`„${title}“ gibt es noch nicht.`);
    if (r.created) { await this.refresh(); await this.refreshAllIds(); }
    this.openNote(r.id);
  }

  openLink(url) {
    if (!url) return;
    const m = url.match(/^folio:\/\/note\/([0-9A-Fa-f-]{36})/);
    if (m) return this.openNote(m[1].toUpperCase());
    const w = url.match(/^folio:\/\/wiki\/(.+)$/);
    if (w) return this.openWiki(decodeURIComponent(w[1]), true);
    if (url.startsWith('folio://')) return this.deepLink(url);
    if (inApp) call('open_url', { url }).catch(showError);
    else window.open(url, '_blank', 'noopener');
  }

  async deepLink(url) {
    try {
      const a = await call('deep_link', { url });
      if (a.action === 'openNote') this.openNote(a.id);
      else if (a.action === 'search') { this.select({ type: 'all' }); this.setQuery(a.query); }
      else if (a.action === 'capture') Panels.quickCapture(this, a.text, a.target);
      else if (a.action === 'newNote') { if (await confirmDialog('Neue Notiz anlegen?', `Ein Link möchte die Notiz „${a.title || 'Neue Notiz'}“ anlegen.`, { ok: 'Anlegen' })) this.newNote({ title: a.title, text: a.text }); }
    } catch (e) { showError(e); }
  }

  attVersion(id) { return this.attVersions[id] || ''; }
  bumpAtt(id) { this.attVersions[id] = String(Date.now()); }

  async openAttachment(noteId, attId) {
    try { await this.editors[noteId]?.flush(); await call('attachment_open', { id: noteId, att: attId }); } catch (e) { showError(e); }
  }

  async exportAttachment(noteId, attId) {
    const info = await call('attachment_info', { id: noteId, att: attId });
    const dest = await saveDialog({ defaultPath: info.filename, title: 'Anhang sichern' });
    if (!dest) return;
    try { await call('attachment_export', { id: noteId, att: attId, dest }); toast('Gesichert'); } catch (e) { showError(e); }
  }

  /** Inserts a file (dropped, pasted or chosen) as the matching block. */
  async insertFileData(ed, name, buf, path) {
    try {
      const att = path ? await call('attachment_add', { id: ed.id, path }) : await call('attachment_add', { id: ed.id, filename: name, data: b64(buf) });
      ed.note.attachments.push(att);
      const block = { id: crypto.randomUUID().toUpperCase(), kind: att.kind, attachmentID: att.id };
      if (att.kind === 'image') block.width = 1;
      if (att.kind === 'pdf') { block.height = 520; block.width = 1; }
      if (att.kind === 'file') { block.height = 420; block.width = 1; }
      ed.insertBlock(block);
    } catch (e) { showError(e); }
  }

  async insertFiles(ed) {
    const paths = await openDialog({ multiple: true, title: 'Datei, PDF oder Bild einfügen' });
    if (!paths) return;
    for (const p of [].concat(paths)) {
      try {
        const att = await call('attachment_add', { id: ed.id, path: p });
        ed.note.attachments.push(att);
        ed.insertBlock({ id: crypto.randomUUID().toUpperCase(), kind: att.kind, attachmentID: att.id, width: 1, height: att.kind === 'pdf' ? 520 : att.kind === 'file' ? 420 : undefined });
      } catch (e) { showError(e); }
    }
  }

  // MARK: Insert menu and formatting

  async insert(kind, ed = this.currentEditor()) {
    if (!ed) return;
    const id = crypto.randomUUID().toUpperCase();
    switch (kind) {
      case 'code': return ed.insertBlock({ id, kind: 'code', code: '', language: this.lastLanguage || 'python' });
      case 'math': return ed.insertBlock({ id, kind: 'math', code: '' });
      case 'diagram': return ed.insertBlock({ id, kind: 'diagram', code: 'graph TD\n  A[Start] --> B[Ziel]' });
      case 'database': return ed.insertBlock({ id, kind: 'database', code: '', language: 'sql', height: 260, width: 1 });
      case 'audio': return ed.insertBlock({ id, kind: 'audio' });
      case 'drawing': {
        const data = { id, kind: 'image', mode: 'drawing', width: 1 };
        drawingEditor(ed, data, () => ed.insertBlock(data));
        return;
      }
      case 'web': {
        const url = await prompt('Webseite einbetten', { placeholder: 'https://…' });
        if (!url) return;
        const u = /^https?:\/\//i.test(url) ? url : 'https://' + url;
        return ed.insertBlock({ id, kind: 'web', url: u, mode: 'page', height: 480, width: 1 });
      }
      case 'file': return this.insertFiles(ed);
      case 'embed': return Panels.quickOpen(this, 'embed');
      case 'link': {
        const v = ed.activeText();
        if (!v) return;
        const sel = v.selection();
        const url = await prompt('Link', { placeholder: 'https://… oder folio://note/…', label: sel && sel.start === sel.end ? 'Ohne markierten Text wird die Adresse selbst eingefügt.' : '' });
        if (url === null) return;
        v.setLink(url ? (/^[a-z]+:/i.test(url) ? url : 'https://' + url) : null);
        return;
      }
      case 'exportpdf': return this.print();
    }
  }

  textFocused(view) { this.lastFocusedPane = view.el.closest('#pane-right') ? 'right' : 'left'; this.updateFormatBar(view); }
  codeFocused() { this.updateFormatBar(null); }
  selectionChanged(view) { this.updateFormatBarState(view); }

  updateFormatBar(view) {
    const bar = document.getElementById('format-bar');
    const ed = this.currentEditor();
    if (!ed || this.graphShown || this.specialShown) { bar.classList.add('hidden'); return; }
    if (!bar.dataset.built) this.buildFormatBar(bar);
    bar.classList.remove('hidden');
    const main = document.getElementById('main').getBoundingClientRect();
    bar.style.left = (main.left + main.width / 2 - bar.offsetWidth / 2) + 'px';
    bar.style.bottom = '16px';
    if (view) this.updateFormatBarState(view);
  }

  buildFormatBar(bar) {
    bar.dataset.built = '1';
    const t = () => this.currentEditor()?.activeText();
    const btn = (label, title, fn, cls = '') => h('button', { title, class: cls, onmousedown: e => e.preventDefault(), onclick: fn }, label);
    const style = h('select', { title: 'Stil', onmousedown: () => { this._keepSel = t()?.selection(); }, onchange: e => { const v = t(); if (v) v.applyStyle(e.target.value); } },
      ...[['title', 'Titel'], ['heading', 'Überschrift'], ['subheading', 'Unterüberschrift'], ['body', 'Text'], ['monospaced', 'Monospace'], ['quote', 'Zitat']].map(([v, l]) => h('option', { value: v }, l)));
    this.styleSelect = style;
    this.fmtButtons = {
      b: btn('B', 'Fett (⌘B)', () => t()?.format('bold')),
      i: btn('I', 'Kursiv (⌘I)', () => t()?.format('italic')),
      u: btn('U', 'Unterstrichen (⌘U)', () => t()?.format('underline')),
      s: btn('S', 'Durchgestrichen (⇧⌘X)', () => t()?.format('strikeThrough')),
    };
    this.fmtButtons.b.style.fontWeight = '800';
    this.fmtButtons.i.style.fontStyle = 'italic';
    this.fmtButtons.u.style.textDecoration = 'underline';
    this.fmtButtons.s.style.textDecoration = 'line-through';
    bar.append(style, h('span', { class: 'sep' }), ...Object.values(this.fmtButtons), h('span', { class: 'sep' }),
      btn('•', 'Aufzählung (⇧⌘7)', () => t()?.toggleList('bullet')),
      btn('1.', 'Nummerierte Liste (⇧⌘8)', () => t()?.toggleList('numbered')),
      btn('☑', 'Checkliste (⇧⌘L)', () => t()?.toggleList('checklist')),
      btn('⇥', 'Einrücken (⌘])', () => t()?.indent(1)),
      h('span', { class: 'sep' }),
      btn('🔗', 'Link (⌘K)', () => this.insert('link')),
      btn('{ }', 'Code-Block (⌥⌘C)', () => this.insert('code')),
      btn('＋', 'Einfügen', e => menuAt(e.currentTarget, this.insertItems()).style && 0),
      btn('✦', 'KI', e => this.aiMenu(e.currentTarget)),
      btn('▶︎', 'Alle Code-Blöcke ausführen (⌃⇧R)', () => this.currentEditor()?.runAll()));
  }

  insertItems() {
    return [
      { label: 'Code-Block', detail: '⌥⌘C', action: () => this.insert('code') },
      { label: 'Datei, PDF oder Bild …', detail: '⇧⌘A', action: () => this.insert('file') },
      { label: 'Datenbank-Abfrage', action: () => this.insert('database') },
      { label: 'Formel (LaTeX)', detail: '⌥⌘M', action: () => this.insert('math') },
      { label: 'Diagramm (Mermaid)', detail: '⌥⌘G', action: () => this.insert('diagram') },
      { label: 'Notiz einbetten …', action: () => this.insert('embed') },
      { label: 'Audioaufnahme', detail: '⌥⌘R', action: () => this.insert('audio') },
      { label: 'Zeichnung', detail: '⌥⌘D', action: () => this.insert('drawing') },
      { label: 'Webseite …', detail: '⌥⌘L', action: () => this.insert('web') },
    ];
  }

  updateFormatBarState(view) {
    if (!this.fmtButtons || !view) return;
    const q = c => { try { return document.queryCommandState(c); } catch { return false; } };
    this.fmtButtons.b.classList.toggle('on', q('bold'));
    this.fmtButtons.i.classList.toggle('on', q('italic'));
    this.fmtButtons.u.classList.toggle('on', q('underline'));
    this.fmtButtons.s.classList.toggle('on', q('strikeThrough'));
    this.styleSelect.value = view.currentStyle();
  }

  textContextMenu(e, view) {
    const sel = view.selectedText();
    if (!sel) return;
    e.preventDefault();
    menu(e.clientX, e.clientY, [
      { label: 'Ausschneiden', action: () => document.execCommand('cut') },
      { label: 'Kopieren', action: () => document.execCommand('copy') },
      '-',
      { heading: 'KI' },
      ...['explain', 'simplify', 'rewrite', 'quiz'].map(a => ({ label: { explain: 'Erklären', simplify: 'Vereinfachen', rewrite: 'Umschreiben', quiz: 'Lernfragen erstellen' }[a], action: () => Panels.aiAction(this, view, a, sel) })),
      '-',
      { label: `„${sel.slice(0, 24)}${sel.length > 24 ? '…' : ''}“ als Notiz verlinken`, action: () => { view.replaceSelection(`[[${sel}]]`); } },
    ]);
  }

  aiMenu(anchor) {
    const ed = this.currentEditor();
    const v = ed && ed.activeText();
    const sel = v ? v.selectedText() : '';
    menuAt(anchor, [
      { label: 'Chat über diese Notiz', detail: '⌥⌘I', action: () => Panels.chat(this, 'note') },
      { label: 'Bibliothek fragen …', detail: '⇧⌥⌘I', action: () => Panels.chat(this, 'library') },
      { label: 'Tags vorschlagen', action: () => Panels.suggestTags(this) },
      '-',
      ...['explain', 'simplify', 'rewrite', 'quiz'].map(a => ({ label: { explain: 'Markierung erklären', simplify: 'Markierung vereinfachen', rewrite: 'Markierung umschreiben', quiz: 'Lernfragen zur Markierung' }[a], disabled: !sel, action: () => Panels.aiAction(this, v, a, sel) })),
    ]);
  }

  async print() {
    await this.currentEditor()?.flush();
    if (inApp) native('print').catch(() => window.print()); else window.print();
  }

  async exportMarkdown(id = this.active) {
    if (!id) return;
    await this.editors[id]?.flush();
    const n = this.noteSummary(id) || {};
    const dest = await saveDialog({ defaultPath: (n.title || 'Notiz').replace(/[/:]/g, '-') + '.md', title: 'Als Markdown exportieren', filters: [{ name: 'Markdown', extensions: ['md'] }] });
    if (!dest) return;
    try { await call('markdown_export', { id, dest }); toast('Exportiert'); } catch (e) { showError(e); }
  }

  async importMarkdown() {
    const paths = await openDialog({ multiple: true, title: 'Markdown importieren', filters: [{ name: 'Markdown', extensions: ['md', 'markdown', 'txt'] }] });
    if (!paths) return;
    try {
      const ids = await call('markdown_import', { paths: [].concat(paths), folder: this.selection.type === 'folder' ? this.selection.value : undefined });
      toast(`${ids.length} Notiz${ids.length === 1 ? '' : 'en'} importiert`);
      await this.refresh();
      if (ids[0]) this.openNote(ids[0]);
    } catch (e) { showError(e); }
  }

  async importAppleNotes() {
    if (!(await confirmDialog('Aus Apple Notizen importieren?', 'Folio liest alle Notizen aus der App „Notizen“ und legt sie im Ordner „Apple Notizen“ ab. macOS fragt beim ersten Mal nach der Erlaubnis.', { ok: 'Importieren' }))) return;
    const t = toast('Importiere aus Apple Notizen …', { timeout: 60000 });
    try { const n = await call('apple_notes_import', {}); t.remove(); toast(`${n} Notizen importiert`); this.refresh(); } catch (e) { t.remove(); showError(e); }
  }

  async runEnvironment(ed) { Panels.runEnvironment(this, ed || this.currentEditor()); }

  showGraph(center) { this.graphShown = true; this.updateFormatBar(); Panels.graph(this, center); }

  // MARK: Commands (menus and shortcuts)

  async command(name, arg) {
    const ed = this.currentEditor();
    const t = ed && ed.activeText();
    switch (name) {
      case 'new-note': return this.newNote();
      case 'new-tab': return this.newNote({ newTab: true });
      case 'new-folder': return this.newFolder(this.selection.type === 'folder' ? this.selection.value : '');
      case 'daily': return this.dailyNote();
      case 'capture': return Panels.quickCapture(this);
      case 'close-tab': return this.closeTab();
      case 'next-tab': return this.selectTab(1);
      case 'prev-tab': return this.selectTab(-1);
      case 'split': return this.split ? this.openSplit(null) : (this.active && Panels.quickOpen(this, 'split'));
      case 'window': return this.active && native('open_note_window', { id: this.active });
      case 'duplicate': return this.duplicate();
      case 'delete': return this.deleteNote();
      case 'save': return ed && (ed.dirty = true, ed.save());
      case 'quick-open': return Panels.quickOpen(this, 'open');
      case 'link-note': return Panels.quickOpen(this, 'link');
      case 'history': return this.active && Panels.history(this, this.active);
      case 'info': return this.active && Panels.info(this, this.active);
      case 'print': case 'export-pdf': return this.print();
      case 'export-md': return this.exportMarkdown();
      case 'import-md': return this.importMarkdown();
      case 'import-apple': return this.importAppleNotes();
      case 'template-save': return this.active && (await ed?.flush(), await call('template_from_note', { id: this.active }), toast('Als Vorlage gesichert'), this.refresh());
      case 'templates': return this.select({ type: 'templates' });
      case 'new-from-template': return Panels.templatePicker(this);
      case 'find': return ed && ed.openFind(false);
      case 'replace': return ed && ed.openFind(true);
      case 'search': { const s = document.getElementById('search'); document.body.classList.remove('sidebar-hidden', 'focus-mode'); s.focus(); s.select(); return; }
      case 'toggle-sidebar': return document.body.classList.toggle('sidebar-hidden');
      case 'focus': document.body.classList.toggle('focus-mode'); return this.updateFormatBar();
      case 'graph': return this.select({ type: 'graph' });
      case 'run-env': return this.runEnvironment();
      case 'run-all': return ed && ed.runAll();
      case 'settings': return Panels.settings(this, arg);
      case 'shortcuts': return Panels.settings(this, 'shortcuts');
      case 'chat': return Panels.chat(this, 'note');
      case 'ask-library': return Panels.chat(this, 'library');
      case 'bold': return t && t.format('bold');
      case 'italic': return t && t.format('italic');
      case 'underline': return t && t.format('underline');
      case 'strike': return t && t.format('strikeThrough');
      case 'mono': return t && t.toggleMono();
      case 'style': return t && t.applyStyle(arg);
      case 'list': return t && t.toggleList(arg);
      case 'indent': return t && t.indent(1);
      case 'outdent': return t && t.indent(-1);
      case 'font-bigger': return t && t.fontSize(2);
      case 'font-smaller': return t && t.fontSize(-2);
      case 'align': return t && t.align(arg);
      case 'undo': if (ed && ed.undo()) return; return document.execCommand('undo');
      case 'redo': if (ed && ed.redo()) return; return document.execCommand('redo');
      case 'insert': return this.insert(arg);
      case 'deep-link': return this.deepLink(arg);
      case 'open-note': return this.openNote(arg);
      case 'backup-now': try { await call('backup_now'); toast('Sicherung erstellt'); } catch (e) { showError(e); } return;
      case 'help': return this.openLink('https://github.com/gigafrosxh/folio-rust/blob/main/README.md');
      case 'privacy': return this.openLink('https://github.com/gigafrosxh/folio-rust/blob/main/docs/PRIVACY.md');
      case 'changelog': return this.openLink('https://github.com/gigafrosxh/folio-rust/blob/main/CHANGELOG.md');
      case 'issues': return this.openLink('https://github.com/gigafrosxh/folio-rust/issues/new');
      case 'about': return toast(`Folio ${this.version} – in Rust`);
      case 'reload': return this.refresh();
      case 'flush': return this.flushAll();
    }
  }

  bindStatic() {
    document.getElementById('btn-new-note').onclick = () => this.newNote();
    document.getElementById('btn-new-folder').onclick = () => this.command('new-folder');
    document.getElementById('btn-settings').onclick = () => this.command('settings');
    const search = document.getElementById('search');
    const run = debounce(() => { this.query = search.value; this.refreshList(); }, 120);
    search.addEventListener('input', run);
    search.addEventListener('keydown', e => {
      if (e.key === 'Escape') { search.value = ''; this.setQuery(''); search.blur(); }
      if (e.key === 'ArrowDown' || e.key === 'Enter') { e.preventDefault(); if (this.notes[0]) this.openNote(this.notes[0].id); document.getElementById('note-list').focus(); }
    });
    document.getElementById('btn-filter').onclick = e => Panels.filterMenu(this, e.currentTarget);
    document.getElementById('btn-sort').onclick = e => {
      const m = this.library.sortMode;
      menuAt(e.currentTarget, [['modified', 'Zuletzt bearbeitet'], ['created', 'Erstellungsdatum'], ['title', 'Titel'], ['manual', 'Eigene Reihenfolge']].map(([k, l]) => ({ label: l, checked: m === k, action: async () => { await call('sort_mode', { mode: k }); this.refresh(); } }))
        .concat(['-', { label: 'Unterordner einbeziehen', checked: this.settings.includeSubfolders, action: () => this.updateSettings({ includeSubfolders: !this.settings.includeSubfolders }) },
          { label: 'Suche in der ganzen Bibliothek', checked: this.settings.searchEverywhere, action: () => this.updateSettings({ searchEverywhere: !this.settings.searchEverywhere }) }]));
    };
    const list = document.getElementById('note-list');
    list.addEventListener('keydown', e => {
      const i = this.notes.findIndex(n => n.id === this.active);
      if (e.key === 'ArrowDown' && this.notes[i + 1]) { e.preventDefault(); this.openNote(this.notes[i + 1].id); }
      if (e.key === 'ArrowUp' && i > 0) { e.preventDefault(); this.openNote(this.notes[i - 1].id); }
      if (e.key === 'Enter' || e.key === 'ArrowRight') { e.preventDefault(); this.currentEditor()?.focusFirst(); }
      if (e.key === 'Backspace' && e.metaKey === false && this.active) { e.preventDefault(); this.deleteNote(this.active); }
    });
    document.getElementById('pane-right').addEventListener('focusin', () => this.lastFocusedPane = 'right');
    document.getElementById('pane-left').addEventListener('focusin', () => this.lastFocusedPane = 'left');
    window.addEventListener('resize', () => this.updateFormatBar());
    document.addEventListener('keydown', e => this.onKey(e), true);
    onNative(p => {
      if (p.type === 'menu') this.command(p.id, p.arg);
      else if (p.type === 'open-url') this.deepLink(p.url);
      else if (p.type === 'files') p.paths.forEach(path => this.openFile(path));
    });
    window.addEventListener('beforeunload', () => { for (const ed of Object.values(this.editors)) if (ed.dirty) ed.save(); });
    // Pasting files with nothing focused (#app delegate paste) goes into the open note.
    document.addEventListener('paste', async e => {
      if (e.target.closest && (e.target.closest('.rt') || e.target.closest('input, textarea'))) return;
      const ed = this.currentEditor();
      const files = Array.from(e.clipboardData?.files || []);
      if (ed && files.length) { e.preventDefault(); for (const f of files) await this.insertFileData(ed, f.name, await f.arrayBuffer()); }
    });
  }

  async openFile(path) {
    toast('Folio-Dateien außerhalb der Bibliothek: bitte in den Bibliotheksordner legen. ' + path);
  }

  async updateSettings(patch) {
    this.settings = await call('settings_set', { settings: patch });
    this.applySettings();
    this.refresh();
  }

  /** Shortcuts when there is no native menu (tests) plus keys the menu cannot have. */
  onKey(e) {
    const mod = e.metaKey;
    const k = e.key.toLowerCase();
    // Ctrl tool shortcuts like the Swift app (rebindable there; fixed defaults here).
    if (e.ctrlKey && !e.metaKey && !e.altKey) {
      const ctrl = { c: ['insert', 'code'], d: ['insert', 'drawing'], l: ['list', 'checklist'], w: ['insert', 'web'], i: ['insert', 'file'], u: ['insert', 'link'], j: ['chat'] };
      const ctrlShift = { r: ['run-all'], d: ['insert', 'database'], m: ['insert', 'math'], g: ['insert', 'diagram'], a: ['insert', 'audio'] };
      const hit = e.shiftKey ? ctrlShift[k] : ctrl[k];
      if (hit && !(k === 'c' && document.activeElement && document.activeElement.tagName === 'TEXTAREA' && !e.shiftKey)) { e.preventDefault(); this.command(hit[0], hit[1]); return; }
      if (k === '`') { e.preventDefault(); toast('Das Terminal ist in der Rust-Version noch nicht enthalten.'); return; }
    }
    if (mod && k === 'z' && !inApp) { e.preventDefault(); this.command(e.shiftKey ? 'redo' : 'undo'); return; }
    if (inApp) {
      // The native menu has the shortcuts; ⌘Z goes to Folio's own undo too.
      return;
    }
    if (!mod) return;
    const map = {
      'n': e.shiftKey ? 'new-folder' : 'new-note', 't': 'new-tab', 'w': 'close-tab', 'd': e.ctrlKey ? 'daily' : 'duplicate', 's': e.altKey ? 'toggle-sidebar' : 'save',
      'p': 'quick-open', 'f': e.altKey ? 'search' : e.shiftKey ? 'focus' : e.ctrlKey ? 'replace' : 'find', 'k': e.altKey ? 'link-note' : null, 'y': e.altKey ? 'history' : null,
      'b': 'bold', 'i': e.altKey ? (e.shiftKey ? 'ask-library' : 'chat') : 'italic', 'u': 'underline', ',': 'settings', ']': e.shiftKey ? 'next-tab' : 'indent', '[': e.shiftKey ? 'prev-tab' : 'outdent',
      '+': 'font-bigger', '=': 'font-bigger', '-': 'font-smaller', 'e': e.shiftKey ? 'export-pdf' : e.altKey ? 'export-md' : null, '\\': e.altKey ? 'split' : null,
    };
    if (e.shiftKey && !e.altKey) {
      const style = { '1': 'title', '2': 'heading', '3': 'subheading', '0': 'body', '!': 'title', '"': 'heading', '§': 'subheading', '=': 'body' }[e.key];
      if (style) { e.preventDefault(); this.command('style', style); return; }
      if (k === '7' || e.key === '/') { e.preventDefault(); this.command('list', 'bullet'); return; }
      if (k === '8' || e.key === '(') { e.preventDefault(); this.command('list', 'numbered'); return; }
      if (k === 'l') { e.preventDefault(); this.command('list', 'checklist'); return; }
      if (k === 'x') { e.preventDefault(); this.command('strike'); return; }
      if (k === 'a') { e.preventDefault(); this.command('insert', 'file'); return; }
    }
    if (e.altKey && !e.shiftKey) {
      const ins = { c: 'code', m: 'math', g: 'diagram', r: 'audio', d: 'drawing', l: 'web' }[k] || { 'ç': 'code', 'µ': 'math', '©': 'diagram', '®': 'audio', '∂': 'drawing', '@': 'web' }[e.key];
      if (ins) { e.preventDefault(); this.command('insert', ins); return; }
    }
    if (k === 'k' && !e.altKey) { e.preventDefault(); this.command('insert', 'link'); return; }
    const cmd = map[k];
    if (cmd) { e.preventDefault(); this.command(cmd); }
  }
}

const app = new App();
app.start().catch(e => { document.body.innerHTML = `<div class="damaged-view"><h2>Folio konnte nicht starten</h2><pre>${String(e)}</pre></div>`; });
