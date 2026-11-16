// Dialogs and side panels: quick open, settings, history, info, trash, graph, AI chat, capture …

import { call, pollJob, openDialog, inApp, native } from './api.js';
import { h, toast, showError, menu, menuAt, modal, prompt, confirmDialog, fmtDate, fmtSize, debounce } from './ui.js';
import * as RT from './richtext.js';
import { highlight, languages } from './highlight.js';

// MARK: - Quick open (#88): notes, folders, tags; also links and embeds

export function quickOpen(app, mode = 'open') {
  const input = h('input', { class: 'qo-input', placeholder: { open: 'Notiz, Ordner oder #Tag öffnen …', link: 'Notiz verlinken …', embed: 'Notiz einbetten …', split: 'Rechts daneben öffnen …' }[mode] });
  const list = h('div', { class: 'qo-list' });
  let items = [], active = 0;
  const m = modal({ body: h('div', {}, input, list) });
  m.box.querySelector('.body').style.padding = '0';
  const paint = () => {
    list.innerHTML = '';
    items.forEach((it, i) => list.appendChild(h('div', { class: 'item' + (i === active ? ' active' : ''), onclick: () => pick(i), onmousemove: () => { if (active !== i) { active = i; paint(); } } },
      h('span', { class: 'k' }, { note: 'Notiz', folder: 'Ordner', tag: 'Tag', new: 'Neu' }[it.kind]), h('span', {}, it.title), h('span', { class: 'd' }, it.detail))));
  };
  const load = async () => {
    items = await call('switcher', { query: input.value });
    if (mode !== 'open') items = items.filter(i => i.kind === 'note');
    if (mode === 'open' && input.value.trim() && !input.value.startsWith('#') && !input.value.startsWith('/')) items.push({ kind: 'new', title: input.value.trim(), detail: 'Neue Notiz anlegen', key: '' });
    active = 0;
    paint();
  };
  const pick = async i => {
    const it = items[i];
    if (!it) return;
    m.close();
    const ed = app.currentEditor();
    if (mode === 'link') {
      const v = ed && ed.activeText();
      if (v) { v.lastSelection && v.focus(true, v.lastSelection.start); v.insertText(`[[${it.title}]]`); }
      return;
    }
    if (mode === 'embed') { ed && ed.insertBlock({ id: crypto.randomUUID().toUpperCase(), kind: 'embed', code: it.title }); return; }
    if (mode === 'split') { app.openSplit(it.key); return; }
    if (it.kind === 'note') app.openNote(it.key);
    else if (it.kind === 'folder') app.select({ type: 'folder', value: it.key });
    else if (it.kind === 'tag') app.select({ type: 'tag', value: it.key });
    else if (it.kind === 'new') app.newNote({ title: it.title });
  };
  input.addEventListener('input', debounce(load, 60));
  input.addEventListener('keydown', e => {
    if (e.key === 'ArrowDown') { e.preventDefault(); active = Math.min(items.length - 1, active + 1); paint(); }
    if (e.key === 'ArrowUp') { e.preventDefault(); active = Math.max(0, active - 1); paint(); }
    if (e.key === 'Enter') { e.preventDefault(); pick(active); }
  });
  load();
  setTimeout(() => input.focus(), 20);
}

// MARK: - Filter menu (#8)

export function filterMenu(app, anchor) {
  const f = app.filter;
  const tags = app.library.tags.slice(0, 30);
  const toggleSet = (k, v) => { const s = new Set(f[k] || []); s.has(v) ? s.delete(v) : s.add(v); f[k] = [...s]; if (!f[k].length) delete f[k]; app.refreshList(); };
  menuAt(anchor, [
    { heading: 'Zeitraum' },
    ...[['any', 'Beliebig'], ['today', 'Heute'], ['week', 'Letzte 7 Tage'], ['month', 'Letzte 30 Tage'], ['year', 'Letztes Jahr']].map(([k, l]) => ({ label: l, checked: (f.period || 'any') === k, action: () => { if (k === 'any') delete f.period; else f.period = k; app.refreshList(); } })),
    '-', { heading: 'Inhalt' },
    ...app.contentTypes.map(t => ({ label: t.label, checked: (f.types || []).includes(t.id), action: () => toggleSet('types', t.id) })),
    tags.length ? '-' : null, tags.length ? { heading: 'Tags' } : null,
    ...tags.map(t => ({ label: '#' + t.tag, checked: (f.tags || []).includes(t.tag), action: () => toggleSet('tags', t.tag) })),
    '-', { label: 'Filter zurücksetzen', action: () => { app.filter = {}; app.refreshList(); } },
  ]);
}

// MARK: - Settings

export function settings(app, tab = 'general') {
  const s = { ...app.settings };
  const tabs = [['general', 'Allgemein'], ['editor', 'Editor'], ['code', 'Code'], ['ai', 'KI'], ['library', 'Bibliothek & Sicherung'], ['shortcuts', 'Tastenkürzel'], ['about', 'Über']];
  const pane = h('div', { class: 'pane-s' });
  const tabsEl = h('div', { class: 'tabs' });
  const save = debounce(async patch => { app.settings = await call('settings_set', { settings: patch }); app.applySettings(); app.refresh(); }, 250);
  const field = (label, input, hint) => h('div', { class: 'field' }, h('label', {}, label), input, hint ? h('div', { class: 'hint' }, hint) : null);
  const text = (k, opts = {}) => h('input', { type: opts.type || 'text', value: s[k] ?? '', step: opts.step, min: opts.min, max: opts.max, placeholder: opts.placeholder || '', onchange: e => { s[k] = opts.type === 'number' ? +e.target.value : e.target.value; save({ [k]: s[k] }); } });
  const check = (k, label) => h('label', { class: 'row', style: { margin: '8px 0' } }, h('input', { type: 'checkbox', checked: !!s[k], onchange: e => { s[k] = e.target.checked; save({ [k]: s[k] }); } }), label);
  const select = (k, opts) => h('select', { onchange: e => { s[k] = e.target.value; save({ [k]: s[k] }); } }, ...opts.map(([v, l]) => h('option', { value: v, selected: s[k] === v }, l)));
  const draw = async t => {
    tab = t;
    tabsEl.innerHTML = '';
    for (const [k, l] of tabs) tabsEl.appendChild(h('div', { class: 'item' + (k === t ? ' active' : ''), onclick: () => draw(k) }, l));
    pane.innerHTML = '';
    if (t === 'general') {
      pane.append(
        field('Erscheinungsbild', select('appearance', [['system', 'System'], ['light', 'Hell'], ['dark', 'Dunkel']])),
        field('Akzentfarbe', h('input', { type: 'color', value: s.accentHex, onchange: e => { s.accentHex = e.target.value; save({ accentHex: s.accentHex }); } })),
        field('Schwebende Bedienelemente', select('floatingStyle', [['glass', 'Glas'], ['material', 'Milchglas'], ['solid', 'Deckend']])),
        field('Eckenradius', text('cornerRadius', { type: 'number', min: 0, max: 30 })),
        field('Papierkorb automatisch leeren nach (Tagen, 0 = nie)', text('trashRetentionDays', { type: 'number', min: 0 })),
        check('menuBarIcon', 'Symbol in der Menüleiste (Schnellerfassung, neue Notiz)'),
        check('animations', 'Animationen'));
    } else if (t === 'editor') {
      pane.append(
        field('Schrift', select('editorFont', [['System', 'System (San Francisco)'], ['New York', 'New York'], ['Helvetica Neue', 'Helvetica Neue'], ['Georgia', 'Georgia'], ['Avenir Next', 'Avenir Next'], ['Charter', 'Charter']])),
        field('Schriftgröße', text('editorFontSize', { type: 'number', min: 10, max: 30 })),
        field('Zeilenabstand', text('lineSpacing', { type: 'number', min: 0, max: 20 })),
        field('Breite des Textes (Pixel)', text('editorWidth', { type: 'number', min: 480, max: 1600, step: 20 })),
        check('headingAfterTitle', 'Nach dem Titel geht es mit einer Überschrift weiter'),
        check('spellcheck', 'Rechtschreibprüfung'),
        field('Ordner für Tagesnotizen', text('dailyFolder')),
        field('Vorlage für Tagesnotizen (Titel)', text('dailyTemplate', { placeholder: 'leer = ohne Vorlage' })));
    } else if (t === 'code') {
      const ov = h('div', { class: 'list-box' });
      for (const l of languages().filter(l => l.runPlans && l.runPlans.length)) {
        ov.appendChild(h('div', { class: 'li' }, h('span', { style: { width: '120px' } }, l.name), h('input', { type: 'text', class: 'grow', placeholder: l.runPlans[0].map(c => c.join(' ')).join(' && '), value: (s.interpreterOverrides || {})[l.id] || '', style: { border: '0', background: 'transparent' },
          onchange: e => { const o = { ...(s.interpreterOverrides || {}) }; if (e.target.value.trim()) o[l.id] = e.target.value.trim(); else delete o[l.id]; s.interpreterOverrides = o; save({ interpreterOverrides: o }); } })));
      }
      pane.append(
        field('Code-Schrift', text('codeFont')), field('Code-Schriftgröße', text('codeFontSize', { type: 'number', min: 9, max: 24 })),
        check('showLineNumbers', 'Zeilennummern'),
        field('Zeitlimit für Ausführungen (Sekunden)', text('runTimeout', { type: 'number', min: 1 })),
        field('Ausgabe höchstens (KB)', text('maxOutputKb', { type: 'number', min: 1 })),
        h('h3', {}, 'Eigene Befehle'), h('div', { class: 'hint' }, '{file} ist die Quelldatei, {out} ein kompiliertes Programm. Mehrere Schritte mit &&.'), ov);
    } else if (t === 'ai') {
      const models = h('select', { onchange: e => { s.ollamaModel = e.target.value; save({ ollamaModel: s.ollamaModel }); } }, h('option', { value: s.ollamaModel }, s.ollamaModel || '—'));
      const status = h('div', { class: 'hint' });
      const loadModels = async () => {
        status.textContent = 'Frage Ollama …';
        try {
          const list = await call('ai_models');
          models.innerHTML = '';
          for (const m of list) models.appendChild(h('option', { value: m, selected: m === s.ollamaModel }, m));
          if (!s.ollamaModel && list[0]) { s.ollamaModel = list[0]; save({ ollamaModel: list[0] }); }
          status.textContent = `${list.length} Modelle gefunden.`;
        } catch (e) { status.textContent = String(e); }
      };
      pane.append(
        field('KI', select('aiProvider', [['off', 'Aus'], ['ollama', 'Lokales Ollama']]), 'Apple Intelligence steht nur der Swift-Version zur Verfügung. Ollama läuft auf diesem Mac; Notizen verlassen ihn nicht.'),
        field('Ollama-Adresse', text('ollamaUrl', { placeholder: 'http://localhost:11434' })),
        field('Modell', h('div', { class: 'row' }, models, h('button', { class: 'btn', onclick: loadModels }, 'Modelle laden'))), status);
    } else if (t === 'library') {
      const backups = h('div', { class: 'list-box' });
      const loadBackups = async () => {
        backups.innerHTML = '';
        const list = await call('backup_list');
        if (!list.length) backups.appendChild(h('div', { class: 'li hint' }, 'Noch keine Sicherung.'));
        for (const b of list.slice(0, 12)) backups.appendChild(h('div', { class: 'li' }, h('span', { class: 'grow' }, fmtDate(b.date)), h('span', { class: 'hint' }, fmtSize(b.size)),
          h('button', { class: 'btn', onclick: async () => { const r = await call('backup_verify', { path: b.path }); toast(r.damaged.length ? `${r.damaged.length} beschädigt: ${r.damaged.join(', ')}` : `In Ordnung: ${r.notes} Notizen`); } }, 'Prüfen'),
          h('button', { class: 'btn', onclick: async () => { if (await confirmDialog('Sicherung wiederherstellen?', 'Die Notizen kommen in einen neuen Ordner „Wiederhergestellt …“; nichts wird überschrieben.', { ok: 'Wiederherstellen' })) { const f = await call('backup_restore', { path: b.path }); toast('Wiederhergestellt in ' + f); app.refresh(); } } }, 'Wiederherstellen')));
      };
      pane.append(
        field('Bibliotheksordner', h('div', { class: 'row' }, h('input', { type: 'text', value: s.libraryPath, readonly: true, style: { flex: 1 } }),
          h('button', { class: 'btn', onclick: async () => { const p = await openDialog({ directory: true, title: 'Bibliotheksordner wählen' }); if (p) { await app.flushAll(); s.libraryPath = p; app.settings = await call('settings_set', { settings: { libraryPath: p } }); location.reload(); } } }, 'Ändern …')),
          'Jede Notiz ist eine .folio-Datei; die Swift- und die Rust-Version können dieselbe Bibliothek benutzen.'),
        check('backupsEnabled', 'Automatische Sicherungen'),
        field('Alle (Stunden)', text('backupIntervalHours', { type: 'number', min: 1 })),
        field('Ordner für Sicherungen', text('backupFolder', { placeholder: '~/Library/Application Support/Folio/Backups' })),
        h('div', { class: 'row', style: { margin: '10px 0' } }, h('button', { class: 'btn', onclick: async () => { try { await call('backup_now'); toast('Sicherung erstellt'); loadBackups(); } catch (e) { showError(e); } } }, 'Jetzt sichern')),
        backups,
        h('h3', {}, 'Import'), h('div', { class: 'row' }, h('button', { class: 'btn', onclick: () => app.importMarkdown() }, 'Markdown …'), h('button', { class: 'btn', onclick: () => app.importAppleNotes() }, 'Apple Notizen …')));
      loadBackups();
    } else if (t === 'shortcuts') {
      const rows = [
        ['Notizen', [['⌘N', 'Neue Notiz'], ['⇧⌘N', 'Neuer Ordner'], ['⌃⌘D', 'Tagesnotiz'], ['⌘T', 'Neuer Tab'], ['⌘W', 'Tab schließen'], ['⌘P', 'Schnell öffnen'], ['⌥⌘F', 'Notizen durchsuchen'], ['⌘F / ⌃⌘F', 'Suchen / Ersetzen'], ['⌥⌘\\', 'Rechts daneben öffnen'], ['⇧⌘F', 'Fokusmodus'], ['⌥⌘S', 'Seitenleiste'], ['⌃⌘G', 'Graph'], ['⌥⌘Y', 'Versionsverlauf']]],
        ['Text', [['⌘B / ⌘I / ⌘U', 'Fett / Kursiv / Unterstrichen'], ['⇧⌘X', 'Durchgestrichen'], ['⇧⌘1 / 2 / 3 / 0', 'Titel / Überschrift / Unterüberschrift / Text'], ['⇧⌘7 / ⇧⌘8 / ⇧⌘L', 'Aufzählung / Nummeriert / Checkliste'], ['⌘] / ⌘[', 'Einrücken / Ausrücken'], ['⌘K / ⌥⌘K', 'Link / Link zu Notiz'], ['[[', 'Notiz verlinken mit Vorschlägen']]],
        ['Einfügen', [['⌥⌘C oder ⌃C', 'Code-Block'], ['⇧⌘A oder ⌃I', 'Datei, PDF, Bild'], ['⌥⌘M oder ⌃⇧M', 'Formel'], ['⌥⌘G oder ⌃⇧G', 'Diagramm'], ['⌥⌘R oder ⌃⇧A', 'Audio'], ['⌥⌘D oder ⌃D', 'Zeichnung'], ['⌥⌘L oder ⌃W', 'Webseite'], ['⌃⇧D', 'Datenbank-Abfrage']]],
        ['Code', [['⇧↩ oder ⌘↩', 'Ausführen'], ['⌃C (läuft)', 'Stoppen'], ['⌃⇧R', 'Alle Code-Blöcke ausführen'], ['⇥ / ⇧⇥', 'Einrücken / Ausrücken']]],
        ['KI', [['⌥⌘I', 'Chat über die Notiz'], ['⇧⌥⌘I', 'Bibliothek fragen']]],
      ];
      for (const [title, items] of rows) {
        pane.appendChild(h('h3', {}, title));
        const box = h('div', { class: 'list-box' });
        for (const [k, l] of items) box.appendChild(h('div', { class: 'li' }, h('span', { style: { width: '170px', fontFamily: 'var(--mono)' } }, k), h('span', { class: 'grow' }, l)));
        pane.appendChild(box);
      }
    } else if (t === 'about') {
      pane.append(h('h3', {}, `Folio ${app.version}`), h('p', {}, 'Die Rust-Version von Folio: derselbe Funktionsumfang und dieselben .folio-Dateien wie die Swift-App, mit einem Kern in Rust.'),
        h('p', { class: 'hint' }, `Bibliothek: ${s.libraryPath}`), h('button', { class: 'btn', onclick: () => app.command('issues') }, 'Fehler melden oder Idee vorschlagen …'));
    }
  };
  modal({ title: 'Einstellungen', wide: true, body: h('div', { class: 'settings' }, tabsEl, pane) });
  draw(tab);
}

// MARK: - History (#versions)

export async function history(app, id) {
  await app.editors[id]?.flush();
  const versions = await call('history_list', { id });
  const list = h('div', { class: 'versions list-box', style: { border: 0, borderRadius: 0 } });
  const preview = h('div', { class: 'preview' }, h('div', { class: 'hint' }, versions.length ? 'Version wählen' : 'Noch keine früheren Versionen. Folio legt beim Bearbeiten höchstens alle fünf Minuten eine an.'));
  let chosen = null;
  versions.forEach((v, i) => {
    const row = h('div', { class: 'li', onclick: async () => {
      list.querySelectorAll('.li').forEach(x => x.classList.remove('active'));
      row.classList.add('active');
      chosen = v;
      const p = await call('history_preview', { path: v.path });
      preview.innerHTML = '';
      for (const b of p.blocks) {
        if (b.kind === 'text') { const d = h('div', { class: 'rt' }); RT.render(d, b.rich); preview.appendChild(d); }
        else if (b.kind === 'code') preview.appendChild(h('pre', { class: 'code-out', html: highlight(b.code || '', b.language) }));
        else preview.appendChild(h('div', { class: 'hint' }, `[${b.kind}]`));
      }
    } }, h('span', { class: 'grow' }, fmtDate(v.date)), h('span', { class: 'hint' }, fmtSize(v.size)));
    list.appendChild(row);
  });
  modal({
    title: 'Versionsverlauf', wide: true, body: h('div', { class: 'history' }, list, preview),
    buttons: [{ label: 'Schließen' }, { label: 'Diese Version wiederherstellen', primary: true, action: async () => {
      if (!chosen) return true;
      await call('history_restore', { id, path: chosen.path });
      await app.reopenNote(id);
      app.refresh();
      toast('Version wiederhergestellt; der vorherige Stand ist jetzt selbst eine Version.');
    } }],
  });
}

// MARK: - Info (#19)

export async function info(app, id) {
  await app.editors[id]?.flush();
  const i = await call('note_info', { id });
  const s = i.stats;
  const row = (k, v) => h('div', { class: 'li' }, h('span', { class: 'grow' }, k), h('span', {}, String(v)));
  modal({ title: 'Informationen zur Notiz', body: h('div', { class: 'list-box' },
    row('Erstellt', fmtDate(i.created)), row('Geändert', fmtDate(i.modified)), row('Wörter', s.words), row('Zeichen', s.characters), row('Zeichen ohne Leerzeichen', s.charactersWithoutSpaces),
    row('Absätze', s.paragraphs), row('Lesezeit', s.readingMinutes ? `${s.readingMinutes} min` : '–'), row('Code-Blöcke', s.codeBlocks), row('Codezeilen', s.codeLines),
    row('Anhänge', i.attachments), row('Dateigröße', fmtSize(i.size)), h('div', { class: 'li hint', style: { wordBreak: 'break-all' } }, i.path)), buttons: [{ label: 'Fertig', primary: true }] });
}

// MARK: - Trash and damaged notes

export async function trashView(app) {
  app.specialShown = true;
  app.updateFormatBar();
  const pane = document.getElementById('pane-left');
  const items = await call('trash_list');
  pane.innerHTML = '';
  const box = h('div', { class: 'list-box' });
  const v = h('div', { class: 'editor' }, h('div', { class: 'row' }, h('h2', {}, 'Papierkorb'), h('span', { class: 'spacer' }),
    items.length ? h('button', { class: 'btn danger', onclick: async () => { if (await confirmDialog('Papierkorb leeren?', 'Alle Einträge werden endgültig gelöscht, auch ihre Versionen. Das lässt sich nicht widerrufen.', { ok: 'Endgültig löschen', danger: true })) { await call('trash_empty'); trashView(app); app.refresh(); } } }, 'Papierkorb leeren') : null),
    h('p', { class: 'hint' }, `Einträge werden nach ${app.settings.trashRetentionDays} Tagen endgültig gelöscht.`), box);
  if (!items.length) box.appendChild(h('div', { class: 'li hint' }, 'Der Papierkorb ist leer.'));
  for (const it of items) {
    box.appendChild(h('div', { class: 'li' }, h('span', {}, it.isFolder ? '📁' : '📄'), h('span', { class: 'grow' }, it.title, h('div', { class: 'hint' }, `${it.originalPath} · gelöscht ${fmtDate(Date.parse(it.deletedAt) / 1000)}${it.isFolder ? ` · ${it.noteCount} Notizen` : ''}`)),
      h('button', { class: 'btn', onclick: async () => { try { const r = await call('trash_restore', { id: it.id }); toast('Wiederhergestellt'); await app.refresh(); trashView(app); if (!it.isFolder && r.ids[0]) app.openNote(r.ids[0]); } catch (e) { showError(e); } } }, 'Wiederherstellen'),
      h('button', { class: 'btn danger', onclick: async () => { if (await confirmDialog('Endgültig löschen?', `„${it.title}“ wird endgültig gelöscht.`, { ok: 'Löschen', danger: true })) { await call('trash_delete', { id: it.id }); trashView(app); app.refresh(); } } }, 'Löschen')));
  }
  pane.appendChild(v);
}

export async function damagedView(app) {
  app.specialShown = true;
  app.updateFormatBar();
  const pane = document.getElementById('pane-left');
  const items = await call('damaged_list');
  pane.innerHTML = '';
  const box = h('div', { class: 'list-box' });
  for (const it of items) box.appendChild(h('div', { class: 'li' }, h('span', { class: 'grow' }, it.name, h('div', { class: 'hint' }, it.problem || '')),
    h('button', { class: 'btn', onclick: () => call('reveal', { path: it.path }) }, 'Im Finder'),
    h('button', { class: 'btn', onclick: async () => { await call('damaged_quarantine', { path: it.path }); damagedView(app); app.refresh(); } }, 'Beiseitelegen')));
  pane.appendChild(h('div', { class: 'editor' }, h('h2', {}, 'Beschädigte Notizen'), h('p', { class: 'hint' }, 'Diese Dateien lassen sich nicht lesen. Folio löscht sie nie; „Beiseitelegen“ verschiebt sie nach .folio/damaged.'), box));
}

export async function templatePicker(app) {
  const list = await call('templates_list');
  menu(innerWidth / 2 - 120, 140, [{ heading: 'Neu aus Vorlage' }, ...list.map(t => ({ label: t.title, action: () => app.newNote({ template: t.id }) })), '-', { label: 'Vorlagen verwalten …', action: () => app.select({ type: 'templates' }) }, { label: 'Neue Vorlage', action: async () => { const r = await call('template_new'); await app.select({ type: 'templates' }); app.openNote(r.id); } }]);
}

// MARK: - Graph (#110–#114, #132)

export async function graph(app, center) {
  const pane = document.getElementById('pane-left');
  pane.innerHTML = '';
  const canvas = h('canvas', { id: 'graph-canvas' });
  const opts = { center, depth: 2, folders: false, orphans: false };
  const search = h('input', { type: 'text', placeholder: 'Hervorheben …' });
  const tools = h('div', { class: 'graph-tools floating' },
    h('button', { class: 'btn', onclick: () => { opts.center = undefined; load(); } }, 'Ganzer Graph'),
    h('label', { class: 'row' }, h('input', { type: 'checkbox', onchange: e => { opts.folders = e.target.checked; load(); } }), 'Ordner'),
    h('label', { class: 'row' }, h('input', { type: 'checkbox', onchange: e => { opts.orphans = e.target.checked; load(); } }), 'Nur verwaiste'),
    search, h('span', { class: 'hint', id: 'graph-info' }));
  const wrap = h('div', { class: 'graph-view' }, tools, canvas);
  pane.appendChild(wrap);
  let data = { nodes: [], edges: [] }, view = { x: 0, y: 0, k: 1 }, hover = null, drag = null;
  const ctx = canvas.getContext('2d');
  const size = () => { const r = wrap.getBoundingClientRect(); canvas.width = r.width * devicePixelRatio; canvas.height = r.height * devicePixelRatio; canvas.style.width = r.width + 'px'; canvas.style.height = r.height + 'px'; };
  const accent = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#E5A50A';
  const textColor = getComputedStyle(document.documentElement).getPropertyValue('--text').trim();
  const lineColor = getComputedStyle(document.documentElement).getPropertyValue('--text-3').trim();
  const toScreen = n => [(n.x * view.k + view.x) * devicePixelRatio + canvas.width / 2, (n.y * view.k + view.y) * devicePixelRatio + canvas.height / 2];
  const paint = () => {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const byId = Object.fromEntries(data.nodes.map(n => [n.id, n]));
    const q = search.value.trim().toLowerCase();
    const neighbors = new Set();
    if (hover) for (const [a, b] of data.edges) { if (a === hover.id) neighbors.add(b); if (b === hover.id) neighbors.add(a); }
    ctx.lineWidth = 1 * devicePixelRatio;
    for (const [a, b] of data.edges) {
      const na = byId[a], nb = byId[b];
      if (!na || !nb) continue;
      const hl = hover && (a === hover.id || b === hover.id);
      ctx.strokeStyle = hl ? accent : lineColor;
      ctx.globalAlpha = hl ? 0.9 : 0.35;
      const [x1, y1] = toScreen(na), [x2, y2] = toScreen(nb);
      ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke();
    }
    ctx.globalAlpha = 1;
    for (const n of data.nodes) {
      const [x, y] = toScreen(n);
      const r = (4 + Math.min(10, Math.sqrt(n.degree) * 2)) * devicePixelRatio * Math.max(0.6, Math.min(1.6, view.k));
      const match = q && n.title.toLowerCase().includes(q);
      const dim = (hover && n !== hover && !neighbors.has(n.id)) || (q && !match);
      ctx.globalAlpha = dim ? 0.25 : 1;
      ctx.fillStyle = n.kind === 'folder' ? lineColor : (n.id === center || match || n === hover ? accent : textColor);
      ctx.beginPath();
      if (n.kind === 'folder') ctx.rect(x - r, y - r, 2 * r, 2 * r); else ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
      if (view.k > 0.7 || n === hover || neighbors.has(n.id) || match || data.nodes.length < 60) {
        ctx.fillStyle = textColor;
        ctx.font = `${11 * devicePixelRatio}px -apple-system, sans-serif`;
        ctx.textAlign = 'center';
        ctx.fillText(n.title.slice(0, 40), x, y + r + 12 * devicePixelRatio);
      }
    }
    ctx.globalAlpha = 1;
  };
  const load = async () => {
    data = await call('graph', opts);
    document.getElementById('graph-info').textContent = `${data.nodes.length} Notizen · ${data.edges.length} Verbindungen · ${data.orphans} verwaist`;
    const xs = data.nodes.map(n => n.x), ys = data.nodes.map(n => n.y);
    const w = Math.max(...xs, 1) - Math.min(...xs, 0), hh = Math.max(...ys, 1) - Math.min(...ys, 0);
    const r = wrap.getBoundingClientRect();
    view.k = Math.min(2, Math.max(0.15, Math.min(r.width / (w + 120), r.height / (hh + 120))));
    view.x = -((Math.max(...xs) + Math.min(...xs)) / 2) * view.k;
    view.y = -((Math.max(...ys) + Math.min(...ys)) / 2) * view.k;
    size();
    paint();
  };
  const nodeAt = (cx, cy) => {
    const r = canvas.getBoundingClientRect();
    const px = (cx - r.left) * devicePixelRatio, py = (cy - r.top) * devicePixelRatio;
    let best = null, bd = 14 * devicePixelRatio;
    for (const n of data.nodes) { const [x, y] = toScreen(n); const d = Math.hypot(x - px, y - py); if (d < bd) { bd = d; best = n; } }
    return best;
  };
  canvas.addEventListener('mousemove', e => {
    if (drag) { view.x += (e.clientX - drag.x); view.y += (e.clientY - drag.y); drag = { x: e.clientX, y: e.clientY, moved: true }; paint(); return; }
    const n = nodeAt(e.clientX, e.clientY);
    if (n !== hover) { hover = n; canvas.style.cursor = n ? 'pointer' : 'grab'; paint(); }
  });
  canvas.addEventListener('mousedown', e => { drag = { x: e.clientX, y: e.clientY, moved: false }; });
  canvas.addEventListener('mouseup', e => {
    const moved = drag && drag.moved;
    drag = null;
    if (moved) return;
    const n = nodeAt(e.clientX, e.clientY);
    if (!n) return;
    if (n.kind === 'folder') { app.select({ type: 'folder', value: n.title }); return; }
    if (e.altKey) { opts.center = n.id; center = n.id; load(); return; }
    app.openNote(n.id);
  });
  canvas.addEventListener('wheel', e => { e.preventDefault(); const f = Math.exp(-e.deltaY * 0.002); view.k = Math.max(0.05, Math.min(6, view.k * f)); paint(); }, { passive: false });
  search.addEventListener('input', paint);
  new ResizeObserver(() => { size(); paint(); }).observe(wrap);
  load();
}

// MARK: - AI (#33–#42)

function sidePanel(title) {
  const p = document.getElementById('side-panel');
  p.classList.remove('hidden');
  p.innerHTML = '';
  const body = h('div', { class: 'sp-body' });
  p.append(h('div', { class: 'sp-head' }, title, h('span', { class: 'spacer' }), h('button', { class: 'icon-btn small', onclick: () => p.classList.add('hidden') }, '✕')), body);
  return { panel: p, body };
}

export function chat(app, scope) {
  if (app.settings.aiProvider !== 'ollama') {
    confirmDialog('KI einschalten?', 'Folio nutzt dafür ein lokales Ollama auf diesem Mac (z. B. mit gemma3). Die Notizen verlassen den Mac nicht.', { ok: 'Einstellungen öffnen' }).then(ok => ok && settings(app, 'ai'));
    return;
  }
  const { panel, body } = sidePanel(scope === 'library' ? 'Bibliothek fragen' : 'Chat über die Notiz');
  const history = [];
  const input = h('textarea', { placeholder: scope === 'library' ? 'Frage an alle Notizen …' : 'Frage zu dieser Notiz …' });
  const send = async () => {
    const q = input.value.trim();
    if (!q) return;
    input.value = '';
    body.appendChild(h('div', { class: 'chat-msg user' }, q));
    const ans = h('div', { class: 'chat-msg' }, '…');
    body.appendChild(ans);
    body.scrollTop = body.scrollHeight;
    try {
      await app.currentEditor()?.flush();
      const job = await call('ai_chat', { question: q, scope, note: app.active, history });
      const r = await pollJob(job, s => { ans.textContent = s.text || '…'; body.scrollTop = body.scrollHeight; }, 120);
      if (r.error) { ans.textContent = r.error; ans.classList.add('err'); return; }
      history.push({ role: 'user', content: q }, { role: 'assistant', content: r.text });
      if (r.sources && r.sources.length) {
        const src = h('div', { class: 'src' }, 'Quellen: ');
        r.sources.forEach((s, i) => src.appendChild(h('a', { onclick: () => app.openNote(s.id) }, `[${i + 1}] ${s.title}`)));
        ans.appendChild(src);
      }
      ans.appendChild(h('div', { class: 'src' }, h('a', { onclick: () => insertAnswer(app, r.text) }, 'Als Blöcke in die Notiz einfügen')));
    } catch (e) { ans.textContent = String(e); }
  };
  input.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } });
  panel.appendChild(h('div', { class: 'chat-input' }, input, h('button', { class: 'btn primary', onclick: send }, '↑')));
  setTimeout(() => input.focus(), 30);
}

/** An answer becomes text, code and table blocks (#40). */
export async function insertAnswer(app, text) {
  const ed = app.currentEditor();
  if (!ed) return;
  const segs = await call('ai_segments', { text });
  for (const s of segs) {
    if (s.type === 'code') ed.insertBlock({ id: crypto.randomUUID().toUpperCase(), kind: 'code', code: s.code, language: s.language || 'plain' });
    else if (s.type === 'table') {
      const lines = [s.header.join('\t'), ...s.rows.map(r => r.join('\t'))];
      ed.insertBlock({ id: crypto.randomUUID().toUpperCase(), kind: 'text', rich: { paras: lines.map((l, i) => ({ runs: [{ text: l, b: i === 0 || undefined }] })) } });
    } else {
      const paras = s.text.split('\n').map(line => {
        const m = line.match(/^(\s*)([-*]|\d+\.)\s+(.*)$/);
        const t = m ? '\t'.repeat(Math.floor(m[1].length / 2)) + (m[2].match(/\d/) ? m[2] + ' ' : '• ') + m[3] : line.replace(/^#{1,6}\s+/, '');
        const head = line.match(/^(#{1,3})\s/);
        const runs = t.split(/(\*\*[^*]+\*\*)/).filter(Boolean).map(x => x.startsWith('**') ? { text: x.slice(2, -2), b: true } : { text: x });
        if (head) runs.forEach(r => { r.b = true; r.size = RT.baseSize() * (head[1].length === 1 ? 1.4 : 1.15); });
        return { runs };
      });
      ed.insertBlock({ id: crypto.randomUUID().toUpperCase(), kind: 'text', rich: { paras } });
    }
  }
}

export async function aiAction(app, view, action, text) {
  if (app.settings.aiProvider !== 'ollama') { chat(app, 'note'); return; }
  const labels = { explain: 'Erklären', simplify: 'Vereinfachen', rewrite: 'Umschreiben', quiz: 'Lernfragen' };
  const out = h('div', { class: 'chat-msg', style: { maxHeight: '50vh', overflow: 'auto' } }, '…');
  let result = '';
  const replace = action === 'simplify' || action === 'rewrite';
  const m = modal({ title: labels[action], body: out, buttons: [{ label: 'Verwerfen' }, { label: 'Kopieren', action: () => { navigator.clipboard.writeText(result); return true; } },
    { label: replace ? 'Markierung ersetzen' : 'Unter der Markierung einfügen', primary: true, action: () => {
      if (!result) return true;
      if (replace) view.replaceSelection(result);
      else { const s = view.lastSelection; if (s) view.focus(true, s.end); view.insertRich({ paras: ('\n' + result).split('\n').map(t => ({ runs: t ? [{ text: t }] : [] })) }); }
    } }] });
  try {
    const job = await call('ai_action', { action, text });
    const r = await pollJob(job, s => { out.textContent = s.text || '…'; }, 120);
    result = r.text;
    if (r.error) out.textContent = r.error;
  } catch (e) { out.textContent = String(e); }
  void m;
}

export async function suggestTags(app) {
  const ed = app.currentEditor();
  if (!ed) return;
  await ed.flush();
  const tags = await call('ai_tags', { id: ed.id });
  if (!tags.length) { toast('Keine passenden Tags gefunden.'); return; }
  menu(innerWidth / 2 - 100, 160, [{ heading: 'Tag hinzufügen' }, ...tags.map(t => ({ label: '#' + t, action: () => {
    const last = ed.textBlocks().pop();
    const rich = last.view.getData().rich;
    const lastPara = rich.paras[rich.paras.length - 1];
    if (RT.paraText(lastPara).trim()) rich.paras.push({ runs: [{ text: '#' + t }] }); else lastPara.runs.push({ text: (RT.paraText(lastPara) ? ' ' : '') + '#' + t });
    last.view.setRich(rich);
    ed.markDirty();
  } }))]);
}

// MARK: - Run environment of a note (#25)

export function runEnvironment(app, ed) {
  if (!ed) return;
  const env = ed.note.meta.environment || {};
  const wd = h('input', { type: 'text', class: 'text', value: env.workingDirectory || '', placeholder: '~/Projekte/demo (leer = temporärer Ordner)' });
  const vars = h('textarea', { rows: 4, style: { width: '100%', fontFamily: 'var(--mono)' }, placeholder: 'API_URL=http://localhost:3000' });
  vars.value = Object.entries(env.variables || {}).map(([k, v]) => `${k}=${v}`).join('\n');
  const timeout = h('input', { type: 'number', class: 'text', value: env.timeout || '', placeholder: String(app.settings.runTimeout) });
  const interp = h('textarea', { rows: 3, style: { width: '100%', fontFamily: 'var(--mono)' }, placeholder: 'python=/opt/homebrew/bin/python3.12 {file}' });
  interp.value = Object.entries(env.interpreters || {}).map(([k, v]) => `${k}=${v}`).join('\n');
  const parse = t => Object.fromEntries(t.split('\n').map(l => l.trim()).filter(l => l.includes('=')).map(l => [l.slice(0, l.indexOf('=')).trim(), l.slice(l.indexOf('=') + 1).trim()]));
  modal({ title: 'Ausführungsumgebung der Notiz', body: h('div', {}, h('div', { class: 'field' }, h('label', {}, 'Arbeitsordner'), wd), h('div', { class: 'field' }, h('label', {}, 'Umgebungsvariablen'), vars),
    h('div', { class: 'field' }, h('label', {}, 'Zeitlimit (s)'), timeout), h('div', { class: 'field' }, h('label', {}, 'Eigene Interpreter (Sprache=Befehl)'), interp)),
    buttons: [{ label: 'Abbrechen' }, { label: 'Sichern', primary: true, action: async () => {
      const e = { interpreters: parse(interp.value), workingDirectory: wd.value.trim() || undefined, variables: parse(vars.value), timeout: timeout.value ? +timeout.value : undefined };
      const empty = !Object.keys(e.interpreters).length && !e.workingDirectory && !Object.keys(e.variables).length && !e.timeout;
      await ed.flush();
      await call('note_save', { id: ed.id, environment: empty ? null : e, keepModified: false });
      ed.note.meta.environment = empty ? undefined : e;
      toast('Umgebung gesichert');
    } }] });
}

// MARK: - Quick capture (#87)

export function quickCapture(app, text = '', target = 'inbox') {
  const ta = h('textarea', { rows: 6, style: { width: '100%' }, placeholder: 'Gedanke, Link oder Aufgabe … (⌘↩ sichert)' });
  ta.value = text || '';
  const sel = h('select', {}, h('option', { value: 'inbox' }, 'Inbox'), ...app.notes.slice(0, 30).map(n => h('option', { value: n.id, selected: n.id === target }, n.title || 'Ohne Titel')));
  const images = [];
  ta.addEventListener('paste', async e => {
    for (const f of Array.from(e.clipboardData.files || [])) if (f.type.startsWith('image/')) { e.preventDefault(); const buf = await f.arrayBuffer(); const { b64 } = await import('./ui.js'); images.push(b64(buf)); toast('Bild angehängt'); }
  });
  const m = modal({ title: 'Schnellerfassung', body: h('div', {}, ta, h('div', { class: 'row', style: { marginTop: '8px' } }, h('span', { class: 'hint' }, 'Ziel'), sel)),
    buttons: [{ label: 'Abbrechen' }, { label: 'Sichern', primary: true, action: async () => {
      if (!ta.value.trim() && !images.length) return true;
      try { const r = await call('capture', { text: ta.value, images, target: sel.value }); toast('Erfasst'); await app.reopenIfOpen(r.id); app.refresh(); } catch (e) { showError(e); }
    } }] });
  ta.addEventListener('keydown', e => { if (e.key === 'Enter' && e.metaKey) { e.preventDefault(); m.box.querySelector('.btn.primary').click(); } });
  setTimeout(() => ta.focus(), 30);
}
