// FOLIO_SELFTEST=1 (only with FOLIO_LIBRARY): checks the real window in the background and reports
// "ok"/"FAIL" lines on stdout, then Folio quits. Never touches a real library.

import { call } from './api.js';

const sleep = ms => new Promise(r => setTimeout(r, ms));
async function until(fn, ms = 6000) {
  const t = Date.now();
  while (Date.now() - t < ms) { try { if (await fn()) return true; } catch { /* retry */ } await sleep(50); }
  throw new Error('Zeitüberschreitung');
}

export async function run(app) {
  const lines = [];
  let failed = 0;
  const step = async (name, fn) => {
    const t = performance.now();
    try { await fn(); lines.push(`ok   ${name} (${Math.round(performance.now() - t)} ms)`); }
    catch (e) { failed++; lines.push(`FAIL ${name}: ${e && e.message || e}`); }
  };
  const ed = () => app.currentEditor();
  await step('Bibliothek geladen', async () => { if (!app.library) throw new Error('keine Bibliothek'); });
  let id;
  await step('Neue Notiz', async () => { id = await app.newNote(); await until(() => ed() && ed().id === id); });
  await step('Text, Liste, Checkliste', async () => {
    const v = ed().textBlocks()[0].view;
    v.setRich({ paras: [{ runs: [{ text: 'Selbsttest' }] }, { runs: [{ text: '• eins' }] }, { runs: [{ text: '☐ zwei #selbsttest' }] }] });
    ed().markDirty();
    await ed().save();
    const n = await call('note_open', { id });
    const text = n.blocks.filter(b => b.kind === 'text').map(b => b.rich.paras.map(p => p.runs.map(r => r.text).join('')).join('\n')).join('\n');
    if (!text.includes('☐ zwei #selbsttest') || n.meta.title !== 'Selbsttest') throw new Error(JSON.stringify(n.meta.title));
    if (!n.meta.tags.includes('selbsttest')) throw new Error('Tag fehlt');
  });
  await step('Code-Block ausführen (bash)', async () => {
    const b = ed().insertBlock({ id: crypto.randomUUID().toUpperCase(), kind: 'code', code: 'echo $((6*7))', language: 'bash' });
    const ok = await b.view.run();
    if (!ok || !(b.view.getData().output || '').includes('42')) throw new Error(b.view.getData().output);
  });
  await step('Formel rendern', async () => {
    const b = ed().insertBlock({ id: crypto.randomUUID().toUpperCase(), kind: 'math', code: 'E = mc^2' });
    await until(() => b.wrap.querySelector('.katex'), 4000);
  });
  await step('Suche', async () => {
    await ed().save();
    const r = await call('notes_list', { query: 'tag:selbsttest' });
    if (r.notes.length !== 1) throw new Error(`${r.notes.length} Treffer`);
  });
  await step('Wiki-Link anlegen und folgen', async () => {
    await app.openWiki('Selbsttest Ziel', true);
    await until(() => ed() && ed().note.meta.title === 'Selbsttest Ziel');
  });
  await step('Papierkorb und Wiederherstellen', async () => {
    await app.deleteNote(ed().id);
    const items = await call('trash_list');
    if (!items.length) throw new Error('leer');
    await call('trash_restore', { id: items[0].id });
  });
  await step('Versionsverlauf', async () => {
    const v = await call('history_list', { id });
    if (!v.length) throw new Error('keine Version');
  });
  await step('Graph', async () => {
    const g = await call('graph', {});
    if (!g.nodes.length) throw new Error('leer');
  });
  await step('Markdown-Export', async () => {
    const lib = (await call('init')).library;
    await call('markdown_export', { id, dest: lib + '/.folio/selftest.md' });
  });
  await step('Anhang über folioatt://', async () => {
    const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
    await app.openNote(id);
    const att = await call('attachment_add', { id, filename: 'p.png', data: png });
    ed().note.attachments.push(att);
    const b = ed().insertBlock({ id: crypto.randomUUID().toUpperCase(), kind: 'image', attachmentID: att.id, width: 1 });
    await ed().save();
    const img = b.wrap.querySelector('img');
    await until(() => img.complete && img.naturalWidth === 1, 4000);
  });
  await call('selftest_report', { lines, failed });
}
