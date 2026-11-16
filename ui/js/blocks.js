// Block views. Each returns {el, getData(), focus?(), destroy?()} plus kind-specific actions.

import { call, attURL, pollJob, openDialog, saveDialog } from './api.js';
import { h, toast, showError, menu, menuAt, fmtSize, debounce, prompt, modal, b64, confirmDialog } from './ui.js';
import * as RT from './richtext.js';
import { highlight, languages, spec } from './highlight.js';

export function makeBlock(ed, data) {
  switch (data.kind) {
    case 'text': return textBlock(ed, data);
    case 'code': return codeBlock(ed, data);
    case 'math': return renderedBlock(ed, data, 'math');
    case 'diagram': return renderedBlock(ed, data, 'diagram');
    case 'image': return imageBlock(ed, data);
    case 'pdf': return pdfBlock(ed, data);
    case 'web': return webBlock(ed, data);
    case 'audio': return audioBlock(ed, data);
    case 'drawing': return drawingBlock(ed, data);
    case 'database': return databaseBlock(ed, data);
    case 'embed': return embedBlock(ed, data);
    default: return fileBlock(ed, data);
  }
}

const newId = () => crypto.randomUUID().toUpperCase();

function rangesFor(text, query, cs, ww) {
  if (!query) return [];
  const out = [];
  const norm = s => cs ? s : s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
  const t = norm(text), q = norm(query);
  // NFD can change lengths; map back by searching the original when lengths differ.
  let i = 0;
  while ((i = t.indexOf(q, i)) >= 0) {
    const ok = !ww || (!/[\p{L}\p{N}_]/u.test(t[i - 1] || '') && !/[\p{L}\p{N}_]/u.test(t[i + q.length] || ''));
    if (ok) out.push([i, i + q.length]);
    i += Math.max(1, q.length);
  }
  return t.length === text.length ? out : out.filter(([a, b]) => b <= text.length);
}

const supportsHighlights = typeof CSS !== 'undefined' && CSS.highlights && typeof Highlight !== 'undefined';

// MARK: - Text

function textBlock(ed, data) {
  const el = h('div', { class: 'rt', contenteditable: 'true', spellcheck: String(ed.app.settings.spellcheck !== false), 'data-placeholder': ed.blocks.length === 0 ? 'Schreib etwas …' : '' });
  RT.render(el, data.rich);
  const view = { el, kind: 'text', lastSelection: null, findRanges: [] };
  let composing = false;
  const redecorate = debounce(() => {
    if (composing || document.activeElement !== el) { RT.decorate(el); return; }
    const s = RT.getSelection(el);
    RT.decorate(el);
    if (s) RT.setSelection(el, s.start, s.end);
  }, 900);

  view.getData = () => {
    const d = { ...data };
    d.rich = RT.serialize(el);
    return d;
  };
  view.setRich = (rich, caret) => {
    RT.render(el, rich);
    if (caret !== undefined) RT.setSelection(el, caret.start ?? caret, caret.end ?? caret.start ?? caret);
  };
  view.focus = (atEnd = true, offset) => {
    el.focus();
    const len = RT.plainText(RT.serialize(el)).length;
    RT.setSelection(el, offset !== undefined ? Math.min(offset, len) : atEnd ? len : 0);
  };
  view.selection = () => RT.getSelection(el) || view.lastSelection;

  /** Runs a model change on the paragraphs of the selection and restores the selection. */
  view.edit = (fn) => {
    ed.pushUndo();
    const sel = RT.getSelection(el) || view.lastSelection || { start: 0, end: 0 };
    const rich = RT.serialize(el);
    const [p1] = RT.locate(rich, sel.start);
    const [p2] = RT.locate(rich, sel.end);
    const before = rich.paras.map(RT.paraText);
    const o1 = sel.start - RT.globalOffset(rich, p1, 0);
    const o2 = sel.end - RT.globalOffset(rich, p2, 0);
    fn(rich, p1, p2, sel);
    // Changes happen at paragraph starts (markers, tabs): the selection keeps its place in the text.
    const d1 = RT.paraText(rich.paras[p1]).length - before[p1].length;
    const d2 = RT.paraText(rich.paras[p2]).length - before[p2].length;
    const ns = RT.globalOffset(rich, p1, Math.max(0, o1 + d1));
    const ne = RT.globalOffset(rich, p2, Math.max(0, o2 + d2));
    RT.render(el, rich);
    el.focus();
    RT.setSelection(el, Math.min(ns, ne), Math.max(ns, ne));
    ed.markDirty();
  };

  view.applyStyle = style => view.edit((rich, p1, p2) => { for (let i = p1; i <= p2; i++) RT.applyStyle(rich.paras[i], style); });
  view.toggleList = kind => view.edit((rich, p1, p2) => RT.toggleList(rich, p1, p2, kind));
  view.indent = d => view.edit((rich, p1, p2) => RT.indentParas(rich, p1, p2, d));
  view.align = a => view.edit((rich, p1, p2) => { for (let i = p1; i <= p2; i++) { if (a === 'left') delete rich.paras[i].align; else rich.paras[i].align = a; } });
  view.fontSize = delta => view.edit((rich, p1, p2, sel) => {
    for (let i = p1; i <= p2; i++) rich.paras[i].runs.forEach(r => { r.size = Math.max(8, Math.min(96, (r.size || RT.baseSize()) + delta)); if (Math.abs(r.size - RT.baseSize()) < 0.3) delete r.size; });
  });
  view.currentStyle = () => {
    const s = RT.getSelection(el) || view.lastSelection;
    if (!s) return 'body';
    const rich = RT.serialize(el);
    return RT.styleOf(rich.paras[RT.locate(rich, s.start)[0]]);
  };
  view.format = cmd => {
    ed.pushUndo();
    el.focus();
    document.execCommand('styleWithCSS', false, false);
    document.execCommand(cmd, false, null);
    ed.markDirty();
  };
  view.toggleMono = () => {
    ed.pushUndo();
    const sel = RT.getSelection(el);
    if (!sel || sel.start === sel.end) return;
    const rich = RT.serialize(el);
    // Split runs at the selection edges, then flip the font of the runs inside.
    const [a, ao] = RT.locate(rich, sel.start), [b, bo] = RT.locate(rich, sel.end);
    let allMono = true;
    for (let i = a; i <= b; i++) {
      const p = rich.paras[i];
      const from = i === a ? ao : 0, to = i === b ? bo : RT.paraText(p).length;
      const [l, rest] = RT.splitRuns(p.runs, from);
      const [mid, r] = RT.splitRuns(rest, to - from);
      if (mid.some(x => x.font !== 'mono')) allMono = false;
      p._parts = [l, mid, r];
    }
    for (let i = a; i <= b; i++) {
      const p = rich.paras[i];
      const [l, mid, r] = p._parts;
      mid.forEach(x => { if (allMono) delete x.font; else x.font = 'mono'; });
      p.runs = [...l, ...mid, ...r];
      delete p._parts;
      RT.mergeRuns(p);
    }
    RT.render(el, rich);
    RT.setSelection(el, sel.start, sel.end);
    ed.markDirty();
  };
  view.setLink = url => {
    ed.pushUndo();
    el.focus();
    const sel = RT.getSelection(el) || view.lastSelection;
    if (sel) RT.setSelection(el, sel.start, sel.end);
    if (!url) document.execCommand('unlink');
    else if (sel && sel.start === sel.end) {
      const a = document.createElement('a');
      a.href = url;
      a.textContent = url;
      window.getSelection().getRangeAt(0).insertNode(a);
    } else document.execCommand('createLink', false, url);
    ed.markDirty();
  };
  view.insertText = (text) => {
    el.focus();
    document.execCommand('insertText', false, text);
  };
  view.selectedText = () => {
    const s = RT.getSelection(el);
    if (!s) return '';
    return RT.plainText(RT.serialize(el)).slice(s.start, s.end);
  };
  /** Replaces the selection with plain text (AI results). */
  view.replaceSelection = (text) => {
    ed.pushUndo();
    const s = view.lastSelection || RT.getSelection(el) || { start: 0, end: 0 };
    el.focus();
    RT.setSelection(el, s.start, s.end);
    document.execCommand('insertText', false, text);
    ed.markDirty();
  };
  view.insertRich = (inserted) => {
    ed.pushUndo();
    const sel = RT.getSelection(el) || view.lastSelection || { start: 0, end: 0 };
    const rich = RT.serialize(el);
    deleteGlobal(rich, sel.start, sel.end);
    const [pi, off] = RT.locate(rich, sel.start);
    const p = rich.paras[pi];
    const [l, r] = RT.splitRuns(p.runs, off);
    const ins = inserted.paras.map(x => ({ ...x, runs: x.runs.map(y => ({ ...y })) }));
    let caret;
    if (ins.length === 1) {
      p.runs = [...l, ...ins[0].runs, ...r];
      RT.mergeRuns(p);
      caret = sel.start + RT.paraText(ins[0]).length;
    } else {
      const first = { ...p, runs: [...l, ...ins[0].runs] };
      const lastIns = ins[ins.length - 1];
      const last = { ...lastIns, runs: [...lastIns.runs, ...r] };
      rich.paras.splice(pi, 1, first, ...ins.slice(1, -1), last);
      caret = RT.globalOffset(rich, pi + ins.length - 1, RT.paraText(lastIns).length);
    }
    RT.render(el, rich);
    el.focus();
    RT.setSelection(el, caret);
    ed.markDirty();
  };

  // Find (#9) with the CSS highlight API: the text itself is not touched.
  view.highlightFind = (q, cs, ww) => {
    const text = RT.plainText(RT.serialize(el));
    view.findRanges = rangesFor(text, q, cs, ww);
    paintFind();
    return view.findRanges;
  };
  const domRange = ([a, b]) => {
    const r = document.createRange();
    const pt = g => {
      const paras = Array.from(el.children);
      for (const p of paras) {
        const len = (p.textContent || '').length;
        if (g <= len) {
          const w = document.createTreeWalker(p, NodeFilter.SHOW_TEXT);
          let n;
          while ((n = w.nextNode())) { if (g <= n.nodeValue.length) return [n, g]; g -= n.nodeValue.length; }
          return [p, 0];
        }
        g -= len + 1;
      }
      return [el, 0];
    };
    const [sn, so] = pt(a), [en, eo] = pt(b);
    r.setStart(sn, so); r.setEnd(en, eo);
    return r;
  };
  let currentHit = -1;
  const paintFind = () => {
    if (!supportsHighlights) return;
    const all = CSS.highlights.get('find') || new Highlight();
    const cur = CSS.highlights.get('find-cur') || new Highlight();
    for (const r of view._ranges || []) { all.delete(r); cur.delete(r); }
    view._ranges = view.findRanges.map(domRange);
    view._ranges.forEach((r, i) => (i === currentHit ? cur : all).add(r));
    CSS.highlights.set('find', all);
    CSS.highlights.set('find-cur', cur);
  };
  view.markCurrent = i => {
    currentHit = i;
    paintFind();
    const r = view._ranges && view._ranges[i];
    if (r) { const rect = r.getBoundingClientRect(); if (rect.top < 60 || rect.bottom > window.innerHeight - 60) r.startContainer.parentElement.scrollIntoView({ block: 'center' }); }
  };
  view.clearHighlights = () => { view.findRanges = []; currentHit = -1; paintFind(); };
  view.replaceRange = ([a, b], text) => {
    const rich = RT.serialize(el);
    replaceInRich(rich, a, b, text);
    RT.render(el, rich);
  };
  view.replaceAll = (q, text, cs, ww) => {
    const rich = RT.serialize(el);
    const ranges = rangesFor(RT.plainText(rich), q, cs, ww).reverse();
    for (const [a, b] of ranges) replaceInRich(rich, a, b, text);
    if (ranges.length) RT.render(el, rich);
    return ranges.length;
  };

  // MARK: Events

  el.addEventListener('focus', () => { ed.focusedBlock = data.id; ed.app.textFocused(view); });
  el.addEventListener('blur', () => { view.lastSelection = RT.getSelection(el) || view.lastSelection; });
  el.addEventListener('compositionstart', () => composing = true);
  el.addEventListener('compositionend', () => { composing = false; });
  document.addEventListener('selectionchange', view._sel = () => {
    if (document.activeElement === el) { view.lastSelection = RT.getSelection(el); ed.app.selectionChanged(view); }
  });

  el.addEventListener('input', e => {
    // Normalize stray structure (text directly in the block) back into paragraphs.
    if (Array.from(el.childNodes).some(n => n.nodeType === 3 || (n.nodeType === 1 && !n.classList.contains('p')))) {
      const s = RT.getSelection(el);
      RT.render(el, RT.serialize(el));
      if (s) RT.setSelection(el, s.start, s.end);
    }
    ed.markDirty();
    if (e.inputType === 'insertText' && e.data === ' ') autoList();
    if (!composing) wikiComplete();
    redecorate();
  });

  /** „- “, „* “, „[] “ and „[x] “ at the start of a line become a list marker, like in Apple Notes. */
  const autoList = () => {
    const before = RT.textBeforeCaret(el);
    const m = before.match(/^(\t*)(- |\* |\[\] |\[ \] |\[x\] )$/);
    if (!m) return;
    const rep = { '- ': '• ', '* ': '• ', '[] ': '☐ ', '[ ] ': '☐ ', '[x] ': '☑ ' }[m[2]];
    const s = RT.getSelection(el);
    RT.setSelection(el, s.start - m[2].length, s.start);
    document.execCommand('insertText', false, rep);
  };

  // [[ … completion with note titles (#115).
  let popup = null;
  const closePopup = () => { if (popup) { popup.el.remove(); popup = null; } };
  const wikiComplete = async () => {
    const before = RT.textBeforeCaret(el);
    const m = before.match(/(!?)\[\[([^\]\n|]*)$/);
    if (!m) return closePopup();
    const q = m[2];
    const items = (await call('switcher', { query: q })).filter(x => x.kind === 'note').slice(0, 12);
    if (q && !items.some(i => i.title.toLowerCase() === q.toLowerCase())) items.push({ kind: 'new', title: q, detail: 'neu anlegen', key: '' });
    if (!items.length) return closePopup();
    closePopup();
    const box = h('div', { class: 'popup floating' });
    let active = 0;
    const choose = it => {
      closePopup();
      const s = RT.getSelection(el);
      RT.setSelection(el, s.start - q.length, s.start);
      document.execCommand('insertText', false, it.title + ']]');
      ed.markDirty();
      setTimeout(() => RT.decorate(el), 0);
    };
    const paint = () => {
      box.innerHTML = '';
      items.forEach((it, i) => box.appendChild(h('div', { class: 'item' + (i === active ? ' active' : ''), onmousedown: e => { e.preventDefault(); choose(it); } }, it.title, h('span', { class: 'd' }, it.detail || ''))));
    };
    paint();
    document.getElementById('menu-host').appendChild(box);
    const r = window.getSelection().getRangeAt(0).getBoundingClientRect();
    box.style.left = Math.min(r.left, innerWidth - 300) + 'px';
    box.style.top = (r.bottom + 6) + 'px';
    popup = { el: box, items, move: d => { active = (active + d + items.length) % items.length; paint(); }, pick: () => choose(items[active]) };
  };

  el.addEventListener('keydown', e => {
    if (popup) {
      if (e.key === 'ArrowDown') { e.preventDefault(); popup.move(1); return; }
      if (e.key === 'ArrowUp') { e.preventDefault(); popup.move(-1); return; }
      if (e.key === 'Enter' || e.key === 'Tab') { e.preventDefault(); popup.pick(); return; }
      if (e.key === 'Escape') { e.preventDefault(); closePopup(); return; }
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.metaKey && !e.altKey && !composing) {
      e.preventDefault();
      onEnter();
      return;
    }
    if (e.key === 'Tab' && !e.metaKey && !e.ctrlKey) {
      const before = RT.textBeforeCaret(el);
      const rich = RT.serialize(el);
      const s = RT.getSelection(el);
      const [pi] = RT.locate(rich, s.start);
      if (RT.listInfo(RT.paraText(rich.paras[pi])) || s.start !== s.end || e.shiftKey) {
        e.preventDefault();
        view.indent(e.shiftKey ? -1 : 1);
        return;
      }
      e.preventDefault();
      document.execCommand('insertText', false, '\t');
      return;
    }
    if (e.key === 'Backspace') {
      const s = RT.getSelection(el);
      if (s && s.start === 0 && s.end === 0) { e.preventDefault(); ed.joinWithPrevious(data.id); return; }
    }
  });

  const onEnter = () => {
    ed.pushUndo();
    const s = RT.getSelection(el);
    const rich = RT.serialize(el);
    deleteGlobal(rich, s.start, s.end);
    const [pi, off] = RT.locate(rich, s.start);
    const p = rich.paras[pi];
    const text = RT.paraText(p);
    const info = RT.listInfo(text);
    // Return in an empty list item ends the list.
    if (info && text.length === info.length) {
      RT.deleteRange(p, 0, info.length);
      RT.render(el, rich);
      RT.setSelection(el, RT.globalOffset(rich, pi, 0));
      ed.markDirty();
      return;
    }
    // A line that is just ![[Titel]] becomes an embedded note (#120).
    const embed = text.match(/^!\[\[([^\[\]|\n]+)(?:\|[^\]]*)?\]\]$/);
    if (embed && off === text.length && !(ed.indexOf(data.id) === 0 && pi === 0)) {
      p.runs = [];
      RT.render(el, rich);
      RT.setSelection(el, RT.globalOffset(rich, pi, 0));
      ed.insertBlock({ id: newId(), kind: 'embed', code: embed[1].trim() });
      return;
    }
    const [l, r] = RT.splitRuns(p.runs, off);
    const style = RT.styleOf(p);
    const next = { runs: r };
    if (p.align) next.align = p.align;
    if (p.indent) next.indent = p.indent;
    let caretOff = 0;
    if (info && off >= info.length) {
      const marker = info.indent + RT.nextMarker(info);
      next.runs = [{ text: marker }, ...r];
      caretOff = marker.length;
    }
    // After a heading at its end, the next line is body text (after the title: a heading, #125).
    if (!r.length && style !== 'body' && style !== 'monospaced' && style !== 'quote') {
      const isFirstTitle = style === 'title' && pi === 0 && ed.indexOf(data.id) === 0 && ed.app.settings.headingAfterTitle;
      next._newStyle = isFirstTitle ? 'heading' : 'body';
    }
    p.runs = l;
    rich.paras.splice(pi + 1, 0, next);
    RT.render(el, rich);
    const caret = RT.globalOffset(rich, pi + 1, caretOff);
    RT.setSelection(el, caret);
    if (next._newStyle && next._newStyle !== 'body') {
      // Typing in the empty paragraph uses the heading style through a zero-width placeholder run.
      const span = el.children[pi + 1];
      span.innerHTML = `<span style="font-weight:700;font-size:${(1.4).toFixed(3)}em">​</span>`;
      RT.setSelection(el, caret + 0);
      const t = span.firstChild.firstChild;
      const range = document.createRange();
      range.setStart(t, 1); range.collapse(true);
      const sel = window.getSelection(); sel.removeAllRanges(); sel.addRange(range);
    } else if (next._newStyle === 'body') {
      const span = el.children[pi + 1];
      if (!r.length && !caretOff) {
        span.innerHTML = '<span>​</span>';
        const t = span.firstChild.firstChild;
        const range = document.createRange();
        range.setStart(t, 1); range.collapse(true);
        const sel = window.getSelection(); sel.removeAllRanges(); sel.addRange(range);
      }
    }
    ensureCaretVisible();
    ed.markDirty();
  };

  const ensureCaretVisible = () => {
    const sel = window.getSelection();
    if (!sel.rangeCount) return;
    const rect = sel.getRangeAt(0).getBoundingClientRect();
    const pane = ed.pane.getBoundingClientRect();
    if (rect.bottom > pane.bottom - 80) ed.pane.scrollTop += rect.bottom - pane.bottom + 120;
  };

  el.addEventListener('mousedown', e => {
    // Checkboxes toggle with a click on ☐/☑ (#125).
    const pos = document.caretRangeFromPoint ? document.caretRangeFromPoint(e.clientX, e.clientY) : null;
    if (!pos) return;
    const para = pos.startContainer.nodeType === 3 ? pos.startContainer.parentElement.closest('.p') : pos.startContainer.closest && pos.startContainer.closest('.p');
    if (!para || para.parentNode !== el) return;
    const text = para.textContent;
    const m = text.match(/^(\t*)(☐|☑)/);
    if (!m) return;
    const r = document.createRange();
    r.setStart(para, 0);
    r.setEnd(pos.startContainer, pos.startOffset);
    const off = r.toString().length;
    if (off !== m[1].length && off !== m[1].length + 1) return;
    // Only clicks on the box itself.
    const box = document.createRange();
    const idx = Array.from(el.children).indexOf(para);
    e.preventDefault();
    ed.pushUndo();
    const rich = RT.serialize(el);
    const p = rich.paras[idx];
    const at = m[1].length;
    RT.deleteRange(p, at, at + 1);
    RT.insertTextAt(p, at, m[2] === '☐' ? '☑' : '☐', p.runs[0] ? { ...p.runs[0], text: '' } : { text: '' });
    RT.render(el, rich);
    ed.markDirty();
    void box;
  });

  el.addEventListener('click', e => {
    const wiki = e.target.closest('.wiki');
    if (wiki && (e.metaKey || !wiki.classList.contains('missing') || e.altKey)) {
      e.preventDefault();
      ed.app.openWiki(wiki.dataset.target, true);
      return;
    }
    const tag = e.target.closest('.tag');
    if (tag && e.metaKey) { ed.app.selectTag(tag.dataset.tag); return; }
    const a = e.target.closest('a');
    if (a) {
      e.preventDefault();
      ed.app.openLink(a.getAttribute('href'));
    }
  });

  el.addEventListener('paste', async e => {
    const dt = e.clipboardData;
    if (!dt) return;
    const files = Array.from(dt.files || []);
    if (files.length) {
      e.preventDefault();
      for (const f of files) await ed.app.insertFileData(ed, f.name || 'Bild.png', await f.arrayBuffer());
      return;
    }
    const html = dt.getData('text/html');
    const text = dt.getData('text/plain');
    e.preventDefault();
    if (html) {
      const doc = new DOMParser().parseFromString(html, 'text/html');
      const rich = RT.serialize(doc.body, { linksUnderlined: false });
      // Pasted text adapts to light and dark mode: neutral colors and the page font go.
      for (const p of rich.paras) for (const r of p.runs) {
        if (r.color && isNeutral(r.color)) delete r.color;
        if (r.bg && isWhite(r.bg)) delete r.bg;
        if (r.font && r.font !== 'mono') delete r.font;
        if (r.link && !/^(https?:|mailto:|folio:)/i.test(r.link)) delete r.link;
      }
      while (rich.paras.length > 1 && !RT.paraText(rich.paras[rich.paras.length - 1])) rich.paras.pop();
      view.insertRich(rich);
    } else if (text) {
      view.insertRich({ paras: text.replace(/\r\n?/g, '\n').split('\n').map(t => ({ runs: t ? [{ text: t }] : [] })) });
    }
    redecorate();
  });

  el.addEventListener('drop', async e => {
    const files = Array.from(e.dataTransfer.files || []);
    if (!files.length) return;
    e.preventDefault();
    const pos = document.caretRangeFromPoint(e.clientX, e.clientY);
    if (pos) { const s = window.getSelection(); s.removeAllRanges(); s.addRange(pos); view.lastSelection = RT.getSelection(el); }
    ed.focusedBlock = data.id;
    for (const f of files) await ed.app.insertFileData(ed, f.name, await f.arrayBuffer(), f.path);
  });

  el.addEventListener('contextmenu', e => ed.app.textContextMenu(e, view));
  view.destroy = () => { document.removeEventListener('selectionchange', view._sel); closePopup(); };
  return view;
}

function isNeutral(c) {
  const m = c.match(/^#(..)(..)(..)$/);
  if (!m) return false;
  const [r, g, b] = m.slice(1).map(x => parseInt(x, 16) / 255);
  const br = (r + g + b) / 3, sat = Math.max(r, g, b) - Math.min(r, g, b);
  return sat < 0.08 && (br < 0.2 || br > 0.9);
}
function isWhite(c) { const m = c.match(/^#(..)(..)(..)$/); return m && m.slice(1).every(x => parseInt(x, 16) > 242); }

function deleteGlobal(rich, start, end) {
  if (start >= end) return;
  const [a, ao] = RT.locate(rich, start), [b, bo] = RT.locate(rich, end);
  if (a === b) { RT.deleteRange(rich.paras[a], ao, bo); return; }
  const [l] = RT.splitRuns(rich.paras[a].runs, ao);
  const [, r] = RT.splitRuns(rich.paras[b].runs, bo);
  rich.paras[a].runs = [...l, ...r];
  RT.mergeRuns(rich.paras[a]);
  rich.paras.splice(a + 1, b - a);
}

function replaceInRich(rich, a, b, text) {
  const [pi, off] = RT.locate(rich, a);
  const p = rich.paras[pi];
  const [l, rest] = RT.splitRuns(p.runs, off);
  const fmt = rest[0] || l[l.length - 1] || {};
  deleteGlobal(rich, a, b);
  RT.insertTextAt(rich.paras[pi], off, text, { ...fmt, text: '' });
}

// MARK: - Code (#23–#31)

function codeBlock(ed, data) {
  const app = ed.app;
  let running = null;
  const lang = h('select', { title: 'Sprache', onchange: () => { data.language = lang.value; paint(); ed.markDirty(); } },
    ...languages().map(l => h('option', { value: l.id, selected: (data.language || 'python') === l.id }, l.name)),
    h('option', { value: 'plain', selected: data.language === 'plain' }, 'Text'));
  const runBtn = h('button', { class: 'primary', title: 'Ausführen (⇧↩ oder ⌘↩)', onclick: () => running ? stop() : run() }, '▶ Ausführen');
  const status = h('span', { class: 'hint' });
  const head = h('div', { class: 'embed-head' }, lang, runBtn, status, h('span', { class: 'spacer' }),
    h('button', { title: 'Eingabe (stdin)', onclick: () => { stdinRow.classList.toggle('hidden'); } }, 'stdin'),
    h('button', { title: 'Test', onclick: () => { testRow.classList.toggle('hidden'); } }, 'Test'),
    h('button', { title: 'Kopieren', onclick: () => { navigator.clipboard.writeText(ta.value); toast('Code kopiert'); } }, '⧉'),
    h('button', { title: 'Mehr', onclick: e => menuAt(e.currentTarget, [
      { label: 'Interaktive Eingabe während des Laufs', checked: !!data.interactive, action: () => { data.interactive = !data.interactive || undefined; ed.markDirty(); } },
      { label: 'Ausgabe einklappen', checked: !!data.outputCollapsed, action: () => { data.outputCollapsed = !data.outputCollapsed || undefined; out.classList.toggle('hidden', !!data.outputCollapsed); ed.markDirty(); } },
      { label: 'Ausgabe löschen', action: () => { delete data.output; delete data.outputIsError; showOutput(); ed.markDirty(); } },
      '-',
      { label: 'Ausführungsumgebung der Notiz …', action: () => app.runEnvironment(ed) },
      { label: 'Alle Code-Blöcke ausführen', detail: '⌃⇧R', action: () => ed.runAll() },
    ]) }, '•••'));
  const lines = h('div', { class: 'code-lines' });
  const pre = h('pre');
  const ta = h('textarea', { spellcheck: 'false', autocapitalize: 'off', autocomplete: 'off', wrap: 'off' });
  ta.value = data.code || '';
  const area = h('div', { class: 'code-area' }, pre, ta);
  const wrap = h('div', { class: 'code-wrap' }, app.settings.showLineNumbers !== false ? lines : null, area);
  const stdin = h('textarea', { rows: 2, placeholder: 'Eingabe für das Programm (stdin)', oninput: () => { data.stdin = stdin.value || undefined; ed.markDirty(); } });
  stdin.value = data.stdin || '';
  const live = h('input', { type: 'text', placeholder: 'Eingabe senden (↩)', class: 'hidden', onkeydown: e => { if (e.key === 'Enter' && running) { call('job_send', { job: running, text: live.value + '\n' }); live.value = ''; } } });
  const stdinRow = h('div', { class: 'stdin-row' + (data.stdin ? '' : ' hidden') }, stdin, live);
  const test = data.test || null;
  const expected = h('textarea', { rows: 2, placeholder: 'Erwartete Ausgabe' });
  expected.value = (test && test.expectedOutput) || '';
  const match = h('select', {}, ...[['exact', 'Genau gleich'], ['contains', 'Enthält'], ['regex', 'Regulärer Ausdruck']].map(([v, l]) => h('option', { value: v, selected: test && test.match === v }, l)));
  const exitCode = h('input', { type: 'number', value: test && test.expectedExitCode != null ? test.expectedExitCode : 0, style: { width: '60px' } });
  const testResult = h('span');
  const saveTest = () => {
    data.test = expected.value || exitCode.value !== '0' ? { expectedOutput: expected.value || undefined, expectedExitCode: +exitCode.value, match: match.value } : undefined;
    ed.markDirty();
  };
  [expected, match, exitCode].forEach(x => x.addEventListener('change', saveTest));
  const testRow = h('div', { class: 'test-row' + (test ? '' : ' hidden') }, expected, match, h('span', { class: 'hint' }, 'Exit-Code'), exitCode, testResult,
    h('button', { class: 'btn', onclick: () => { expected.value = (data.output || '').trimEnd(); saveTest(); } }, 'Letzte Ausgabe übernehmen'));
  const out = h('div', { class: 'code-out' + (data.outputCollapsed ? ' hidden' : '') });
  const el = h('div', { class: 'embed-frame' }, head, wrap, stdinRow, testRow, out);

  const paint = () => {
    pre.innerHTML = highlight(ta.value, data.language || 'python');
    const n = ta.value.split('\n').length;
    lines.textContent = Array.from({ length: n }, (_, i) => i + 1).join('\n');
    const hgt = Math.max(1, n) * 1.5 * (app.settings.codeFontSize || 13) + 20;
    area.style.height = hgt + 'px';
    const longest = Math.max(...ta.value.split('\n').map(l => l.length));
    area.style.minWidth = Math.min(4000, longest * 0.62 * (app.settings.codeFontSize || 13) + 30) + 'px';
  };
  const showOutput = (meta) => {
    out.innerHTML = '';
    out.classList.toggle('err', !!data.outputIsError);
    if (meta) out.appendChild(h('span', { class: 'meta' }, meta));
    if (data.output) out.appendChild(document.createTextNode(data.output));
    out.classList.toggle('hidden', !data.output && !meta || !!data.outputCollapsed);
  };
  paint();
  showOutput();

  ta.addEventListener('input', () => { data.code = ta.value; paint(); ed.markDirty(); testResult.textContent = ''; });
  ta.addEventListener('focus', () => { ed.focusedBlock = data.id; app.codeFocused(); });
  ta.addEventListener('keydown', e => {
    if (e.key === 'Enter' && (e.shiftKey || e.metaKey)) { e.preventDefault(); running ? null : run(); return; }
    if (e.key === 'c' && e.ctrlKey && !e.metaKey && running) { e.preventDefault(); stop(); return; }
    if (e.key === 'Tab') {
      e.preventDefault();
      const s = ta.selectionStart, en = ta.selectionEnd, v = ta.value;
      if (s === en && !e.shiftKey) { document.execCommand('insertText', false, '    '); return; }
      const ls = v.lastIndexOf('\n', s - 1) + 1;
      const block = v.slice(ls, en);
      const nb = e.shiftKey ? block.replace(/^( {1,4}|\t)/gm, '') : block.replace(/^/gm, '    ');
      ta.setSelectionRange(ls, en);
      document.execCommand('insertText', false, nb);
      ta.setSelectionRange(ls, ls + nb.length);
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      const s = ta.selectionStart, v = ta.value;
      const ls = v.lastIndexOf('\n', s - 1) + 1;
      const indent = v.slice(ls, s).match(/^[ \t]*/)[0];
      const extra = /[:{(\[]\s*$/.test(v.slice(ls, s)) ? '    ' : '';
      document.execCommand('insertText', false, '\n' + indent + extra);
      return;
    }
    if (e.key === 'Backspace' && !ta.value && ta.selectionStart === 0) { e.preventDefault(); ed.removeBlock(data.id); }
  });

  async function run() {
    if (running) return;
    await ed.save();
    runBtn.textContent = '■ Stopp';
    status.innerHTML = '';
    status.appendChild(h('span', { class: 'running-dot' }));
    data.output = '';
    data.outputIsError = false;
    showOutput('läuft …');
    if (data.interactive) { stdinRow.classList.remove('hidden'); live.classList.remove('hidden'); }
    let result;
    try {
      running = await call('run_code', { code: ta.value, language: data.language || 'python', stdin: data.stdin, interactive: !!data.interactive, note: ed.id });
      result = await pollJob(running, s => {
        data.output = s.output;
        showOutput(s.finished ? null : 'läuft …');
        if (s.serverUrl && !out.querySelector('.server')) out.appendChild(h('a', { class: 'server', onclick: () => app.openLink(s.serverUrl) }, 'Server: ' + s.serverUrl));
      });
    } catch (e) {
      result = { output: String(e), exitCode: -1, finished: true };
    }
    running = null;
    live.classList.add('hidden');
    runBtn.textContent = '▶ Ausführen';
    status.textContent = '';
    data.output = result.output;
    data.outputIsError = result.exitCode !== 0 || result.timedOut || result.cancelled;
    const meta = result.cancelled ? 'abgebrochen' : `${result.exitCode === 0 ? '✓' : '✗ Exit-Code ' + result.exitCode} · ${(result.duration || 0).toFixed(2).replace('.', ',')} s`;
    showOutput(meta);
    if (data.test) {
      const r = await call('check_test', { test: data.test, output: result.output, exitCode: result.exitCode ?? -1 });
      testResult.className = r.ok ? 'badge-ok' : 'badge-fail';
      testResult.textContent = (r.ok ? '✓ ' : '✗ ') + r.message;
    }
    ed.markDirty();
    return !data.outputIsError;
  }
  function stop() { if (running) call('job_cancel', { job: running }); }

  return {
    el, kind: 'code', run,
    getData: () => ({ ...data, code: ta.value }),
    focus: () => { ta.focus(); ta.setSelectionRange(ta.value.length, ta.value.length); },
    highlightFind: (q, cs, ww) => rangesFor(ta.value, q, cs, ww),
    markCurrent: () => {},
    replaceRange: ([a, b], t) => { ta.value = ta.value.slice(0, a) + t + ta.value.slice(b); data.code = ta.value; paint(); },
    replaceAll: (q, t, cs, ww) => { const r = rangesFor(ta.value, q, cs, ww).reverse(); for (const [a, b] of r) ta.value = ta.value.slice(0, a) + t + ta.value.slice(b); data.code = ta.value; paint(); return r.length; },
    clearHighlights: () => {},
    destroy: () => stop(),
  };
}

// MARK: - Formula (KaTeX) and diagram (Mermaid)

let mermaidLoading = null;
function loadMermaid() {
  if (window.mermaid) return Promise.resolve();
  if (!mermaidLoading) mermaidLoading = new Promise((res, rej) => { const s = document.createElement('script'); s.src = 'vendor/mermaid/mermaid.min.js'; s.onload = res; s.onerror = rej; document.head.appendChild(s); });
  return mermaidLoading;
}
let diagramCounter = 0;
const isDark = () => document.documentElement.dataset.theme === 'dark' || (document.documentElement.dataset.theme !== 'light' && matchMedia('(prefers-color-scheme: dark)').matches);

function renderedBlock(ed, data, kind) {
  const outEl = h('div', { class: 'render-out', title: 'Doppelklick zum Bearbeiten' });
  const src = h('textarea', { class: 'src-edit hidden', spellcheck: 'false', rows: 4, placeholder: kind === 'math' ? 'LaTeX, z. B. E = mc^2' : 'Mermaid, z. B. graph TD; A-->B' });
  src.value = data.code || '';
  const toggle = () => { src.classList.toggle('hidden'); if (!src.classList.contains('hidden')) src.focus(); };
  const el = h('div', { class: 'embed-frame' }, h('div', { class: 'embed-head' }, h('span', { class: 'title' }, kind === 'math' ? 'Formel' : 'Diagramm'), h('span', { class: 'spacer' }), h('button', { onclick: toggle }, 'Quelltext')), outEl, src);
  const draw = async () => {
    const code = src.value.trim();
    outEl.classList.remove('err');
    if (!code) { outEl.innerHTML = `<span class="hint">${kind === 'math' ? 'Leere Formel' : 'Leeres Diagramm'} – Doppelklick zum Bearbeiten</span>`; return; }
    try {
      if (kind === 'math') {
        if (!window.katex) await new Promise(r => setTimeout(r, 200));
        window.katex.render(code, outEl, { displayMode: true, throwOnError: true, strict: 'ignore', trust: false });
      } else {
        await loadMermaid();
        window.mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: isDark() ? 'dark' : 'default', fontFamily: '-apple-system, system-ui, sans-serif' });
        await window.mermaid.parse(code);
        const { svg } = await window.mermaid.render('folio-diagram-' + (++diagramCounter), code);
        outEl.innerHTML = svg;
      }
    } catch (e) {
      document.querySelectorAll('[id^="dfolio-diagram-"]').forEach(n => n.remove());
      outEl.classList.add('err');
      outEl.textContent = String((e && e.message) || e);
    }
  };
  const redraw = debounce(draw, 300);
  outEl.addEventListener('dblclick', toggle);
  src.addEventListener('input', () => { data.code = src.value; redraw(); ed.markDirty(); });
  src.addEventListener('focus', () => { ed.focusedBlock = data.id; });
  src.addEventListener('keydown', e => { if (e.key === 'Escape' || (e.key === 'Enter' && e.metaKey)) { e.preventDefault(); src.classList.add('hidden'); } });
  setTimeout(draw, 0);
  if (!data.code) setTimeout(() => toggle(), 50);
  return { el, kind, getData: () => ({ ...data, code: src.value }), focus: () => { src.classList.remove('hidden'); src.focus(); } };
}

// MARK: - Media

function attHead(ed, data, title, extra = []) {
  return h('div', { class: 'embed-head' }, h('span', { class: 'title' }, title), h('span', { class: 'spacer' }), ...extra,
    data.attachmentId || data.attachmentID ? h('button', { title: 'Öffnen', onclick: () => ed.app.openAttachment(ed.id, data.attachmentID) }, 'Öffnen') : null,
    data.attachmentID ? h('button', { title: 'Sichern unter …', onclick: () => ed.app.exportAttachment(ed.id, data.attachmentID) }, 'Sichern …') : null);
}

function attInfo(ed, id) { return (ed.note.attachments || []).find(a => a.id === id) || {}; }

function grip(ed, data, target, min = 120) {
  const g = h('div', { class: 'resize-grip', title: 'Höhe ändern' });
  g.addEventListener('mousedown', e => {
    e.preventDefault();
    const y0 = e.clientY, h0 = target.getBoundingClientRect().height;
    const mv = ev => { const nh = Math.max(min, h0 + ev.clientY - y0); target.style.height = nh + 'px'; data.height = Math.round(nh); };
    const up = () => { removeEventListener('mousemove', mv); removeEventListener('mouseup', up); ed.markDirty(); };
    addEventListener('mousemove', mv);
    addEventListener('mouseup', up);
  });
  return g;
}

function imageBlock(ed, data) {
  const info = attInfo(ed, data.attachmentID);
  const isDrawing = data.mode === 'drawing';
  const img = h('img', { alt: data.altText || data.caption || '', draggable: 'false' });
  img.src = attURL(ed.id, data.attachmentID, ed.app.attVersion(data.attachmentID));
  const box = h('div', { class: 'img-wrap' }, img);
  const applyEdit = () => {
    const e = data.imageEdit || {};
    const turns = ((e.quarterTurns || 0) % 4 + 4) % 4;
    img.style.transform = turns ? `rotate(${turns * 90}deg)` : '';
    img.style.width = Math.round((data.width || 1) * 100) + '%';
    if (turns % 2) { img.style.maxHeight = '70vh'; img.style.margin = '15% auto'; } else { img.style.margin = ''; }
    const c = e.crop;
    if (c && !(c.x === 0 && c.y === 0 && c.width === 1 && c.height === 1)) {
      img.style.clipPath = `inset(${c.y * 100}% ${(1 - c.x - c.width) * 100}% ${(1 - c.y - c.height) * 100}% ${c.x * 100}%)`;
    } else img.style.clipPath = '';
  };
  applyEdit();
  const caption = h('input', { class: 'caption', placeholder: 'Bildunterschrift', value: data.caption || '', oninput: e => { data.caption = e.target.value || undefined; ed.markDirty(); } });
  const size = h('input', { type: 'range', min: 20, max: 100, value: Math.round((data.width || 1) * 100), title: 'Breite', oninput: e => { data.width = +e.target.value / 100; applyEdit(); }, onchange: () => ed.markDirty() });
  const el = h('div', { class: 'embed-frame' }, attHead(ed, data, isDrawing ? 'Zeichnung' : (info.filename || 'Bild'), [
    size,
    isDrawing ? h('button', { onclick: () => drawingEditor(ed, data, () => { img.src = attURL(ed.id, data.attachmentID, ed.app.attVersion(data.attachmentID)); }) }, 'Bearbeiten') : null,
    h('button', { title: 'Drehen', onclick: () => { data.imageEdit = { ...(data.imageEdit || {}), quarterTurns: (((data.imageEdit || {}).quarterTurns || 0) + 1) % 4 }; applyEdit(); ed.markDirty(); } }, '⟳'),
    h('button', { title: 'Beschreibung für VoiceOver', onclick: async () => { const v = await prompt('Bildbeschreibung (Alternativtext)', { value: data.altText || '', label: 'Wird von VoiceOver vorgelesen und beim Export mitgegeben.' }); if (v !== null) { data.altText = v || undefined; img.alt = v; ed.markDirty(); } } }, 'Alt'),
    data.imageEdit && data.imageEdit.crop ? h('button', { title: 'Zuschnitt entfernen (das Original bleibt erhalten)', onclick: () => { delete data.imageEdit.crop; applyEdit(); ed.markDirty(); } }, 'Ganzes Bild') : null,
  ]), box, caption);
  img.addEventListener('dblclick', () => isDrawing ? drawingEditor(ed, data, () => { img.src = attURL(ed.id, data.attachmentID, ed.app.attVersion(data.attachmentID)); }) : ed.app.openAttachment(ed.id, data.attachmentID));
  return { el, kind: 'image', getData: () => ({ ...data }) };
}

function pdfBlock(ed, data) {
  const info = attInfo(ed, data.attachmentID);
  const frame = h('iframe', { class: 'pdf-frame', title: info.filename || 'PDF' });
  frame.src = attURL(ed.id, data.attachmentID);
  frame.style.height = (data.height || 520) + 'px';
  const caption = h('input', { class: 'caption', placeholder: 'Beschreibung', value: data.caption || '', oninput: e => { data.caption = e.target.value || undefined; ed.markDirty(); } });
  const el = h('div', { class: 'embed-frame' }, attHead(ed, data, info.filename || 'PDF', [info.size ? h('span', { class: 'hint' }, fmtSize(info.size)) : null]), frame, grip(ed, data, frame), caption);
  return { el, kind: 'pdf', getData: () => ({ ...data }) };
}

function fileBlock(ed, data) {
  const info = attInfo(ed, data.attachmentID);
  const ext = (info.filename || '').split('.').pop().toUpperCase().slice(0, 5);
  const el = h('div', { class: 'embed-frame' },
    h('div', { class: 'file-card' }, h('div', { class: 'ficon' }, ext || 'DATEI'),
      h('div', { style: { flex: 1, minWidth: 0 } }, h('div', { class: 'fname' }, info.filename || data.caption || 'Anhang fehlt'),
        h('div', { class: 'fmeta' }, info.size != null ? fmtSize(info.size) : '', info.isPackage ? ' · Paket aus der Swift-Version (z. B. Numbers)' : '', data.caption && info.filename ? ' · ' + data.caption : '')),
      data.attachmentID ? h('button', { class: 'btn', onclick: () => ed.app.openAttachment(ed.id, data.attachmentID) }, 'Öffnen') : null,
      data.attachmentID ? h('button', { class: 'btn', onclick: () => ed.app.exportAttachment(ed.id, data.attachmentID) }, 'Sichern …') : null));
  el.addEventListener('dblclick', () => data.attachmentID && ed.app.openAttachment(ed.id, data.attachmentID));
  return { el, kind: data.kind, getData: () => ({ ...data }) };
}

function webBlock(ed, data) {
  const url = data.url || '';
  let host = '';
  try { host = new URL(url).host; } catch { host = url; }
  const body = h('div');
  const caption = h('input', { class: 'caption', placeholder: 'Titel oder Beschreibung', value: data.caption || '', oninput: e => { data.caption = e.target.value || undefined; ed.markDirty(); } });
  const draw = () => {
    body.innerHTML = '';
    if ((data.mode || 'page') === 'page' && /^https?:/i.test(url)) {
      const f = h('iframe', { class: 'web-frame', src: url, sandbox: 'allow-scripts allow-same-origin allow-forms allow-popups', referrerpolicy: 'no-referrer' });
      f.style.height = (data.height || 480) + 'px';
      body.append(f, grip(ed, data, f));
    } else {
      body.appendChild(h('div', { class: 'link-card' }, h('div', { class: 'ficon' }, '🌐'), h('div', { style: { minWidth: 0 } }, h('div', { class: 'fname' }, data.caption || host), h('div', { class: 'lurl' }, url))));
    }
  };
  draw();
  const el = h('div', { class: 'embed-frame' }, h('div', { class: 'embed-head' }, h('span', { class: 'title' }, host || 'Webseite'), h('span', { class: 'spacer' }),
    h('button', { onclick: () => { data.mode = (data.mode || 'page') === 'page' ? 'preview' : 'page'; draw(); ed.markDirty(); } }, (data.mode || 'page') === 'page' ? 'Als Link' : 'Live-Ansicht'),
    h('button', { onclick: async () => { const v = await prompt('Adresse', { value: data.url || '' }); if (v) { data.url = v; ed.markDirty(); ed.app.reopenNote(ed.id); } } }, 'Adresse'),
    h('button', { onclick: () => ed.app.openLink(url) }, 'Im Browser')), body, caption);
  return { el, kind: 'web', getData: () => ({ ...data }) };
}

function audioBlock(ed, data) {
  const info = attInfo(ed, data.attachmentID);
  const body = h('div', { class: 'audio-body' });
  const transcript = h('textarea', { placeholder: 'Mitschrift oder Notizen zur Aufnahme', oninput: e => { data.output = e.target.value || undefined; ed.markDirty(); } });
  transcript.value = data.output || '';
  let rec = null, chunks = [], timer = null;
  const recBtn = h('button', { class: 'primary' }, '● Aufnehmen');
  const drawPlayer = () => {
    body.querySelector('audio') && body.querySelector('audio').remove();
    if (data.attachmentID) body.insertBefore(h('audio', { controls: true, src: attURL(ed.id, data.attachmentID, ed.app.attVersion(data.attachmentID)), preload: 'metadata' }), body.firstChild);
  };
  recBtn.onclick = async () => {
    if (rec) { rec.stop(); return; }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const type = MediaRecorder.isTypeSupported('audio/mp4') ? 'audio/mp4' : 'audio/webm';
      rec = new MediaRecorder(stream, { mimeType: type });
      chunks = [];
      rec.ondataavailable = e => chunks.push(e.data);
      const t0 = Date.now();
      timer = setInterval(() => recBtn.textContent = `■ Stopp ${Math.round((Date.now() - t0) / 1000)} s`, 500);
      rec.onstop = async () => {
        clearInterval(timer);
        stream.getTracks().forEach(t => t.stop());
        const blob = new Blob(chunks, { type });
        const name = `Aufnahme ${new Date().toLocaleString('de-DE').replace(/[/:]/g, '-')}.${type === 'audio/mp4' ? 'm4a' : 'webm'}`;
        const att = await call('attachment_add', { id: ed.id, filename: name, data: b64(await blob.arrayBuffer()), type: type === 'audio/mp4' ? 'com.apple.m4a-audio' : 'org.webmproject.webm' });
        ed.note.attachments.push(att);
        data.attachmentID = att.id;
        rec = null;
        recBtn.textContent = '● Neu aufnehmen';
        drawPlayer();
        ed.markDirty();
      };
      rec.start();
    } catch (e) { showError('Das Mikrofon ist nicht verfügbar: ' + (e.message || e)); }
  };
  body.append(transcript);
  drawPlayer();
  const el = h('div', { class: 'embed-frame' }, attHead(ed, data, info.filename || 'Audio', [recBtn]), body);
  if (!data.attachmentID) recBtn.textContent = '● Aufnahme starten';
  return { el, kind: 'audio', getData: () => ({ ...data }), destroy: () => { if (rec) rec.stop(); } };
}

/** Drawings of the Swift app (PencilKit) cannot be shown here; new drawings are pictures (#drawing mode). */
function drawingBlock(ed, data) {
  const info = attInfo(ed, data.attachmentID);
  const el = h('div', { class: 'embed-frame' }, attHead(ed, data, 'Zeichnung'),
    h('div', { class: 'file-card' }, h('div', { class: 'ficon' }, '✎'), h('div', {}, h('div', { class: 'fname' }, 'Zeichnung aus der Swift-Version'),
      h('div', { class: 'fmeta' }, 'PencilKit-Zeichnungen lassen sich in der Rust-Version nicht anzeigen; sie bleiben unverändert in der Notiz erhalten.', info.size ? ' (' + fmtSize(info.size) + ')' : ''))));
  return { el, kind: 'drawing', getData: () => ({ ...data }) };
}

export function drawingEditor(ed, data, onSaved) {
  const W = 1200, H = 700;
  const canvas = h('canvas', { class: 'draw-canvas', width: W, height: H });
  const ctx = canvas.getContext('2d');
  ctx.fillStyle = '#fff';
  ctx.fillRect(0, 0, W, H);
  if (data.attachmentID) {
    const im = new Image();
    im.onload = () => ctx.drawImage(im, 0, 0, W, H);
    im.src = attURL(ed.id, data.attachmentID, ed.app.attVersion(data.attachmentID));
  }
  let color = '#1f1f24', width = 3, erasing = false, drawing = false, last = null;
  const undo = [];
  const pos = e => { const r = canvas.getBoundingClientRect(); return [(e.clientX - r.left) * W / r.width, (e.clientY - r.top) * H / r.height]; };
  canvas.addEventListener('pointerdown', e => { drawing = true; last = pos(e); undo.push(ctx.getImageData(0, 0, W, H)); canvas.setPointerCapture(e.pointerId); });
  canvas.addEventListener('pointermove', e => {
    if (!drawing) return;
    const p = pos(e);
    ctx.strokeStyle = erasing ? '#fff' : color;
    ctx.lineWidth = erasing ? 24 : width * (e.pressure ? 0.5 + e.pressure : 1);
    ctx.lineCap = 'round';
    ctx.beginPath(); ctx.moveTo(...last); ctx.lineTo(...p); ctx.stroke();
    last = p;
  });
  canvas.addEventListener('pointerup', () => drawing = false);
  const colors = ['#1f1f24', '#d4372c', '#2f9e44', '#1971c2', '#E5A50A', '#9b2393'];
  const tools = h('div', { class: 'row', style: { marginBottom: '8px', flexWrap: 'wrap' } },
    ...colors.map(c => h('button', { class: 'btn', style: { background: c, width: '26px', height: '22px', padding: 0 }, onclick: () => { color = c; erasing = false; } })),
    h('select', { onchange: e => width = +e.target.value }, ...[2, 3, 6, 10].map(w => h('option', { value: w, selected: w === 3 }, w + ' px'))),
    h('button', { class: 'btn', onclick: () => erasing = !erasing }, 'Radierer'),
    h('button', { class: 'btn', onclick: () => { const u = undo.pop(); if (u) ctx.putImageData(u, 0, 0); } }, 'Widerrufen'),
    h('button', { class: 'btn', onclick: () => { undo.push(ctx.getImageData(0, 0, W, H)); ctx.fillStyle = '#fff'; ctx.fillRect(0, 0, W, H); } }, 'Leeren'));
  modal({
    title: 'Zeichnung', wide: true, body: h('div', {}, tools, canvas),
    buttons: [{ label: 'Abbrechen' }, { label: 'Übernehmen', primary: true, action: async () => {
      const blob = await new Promise(r => canvas.toBlob(r, 'image/png'));
      const bytes = b64(await blob.arrayBuffer());
      if (data.attachmentID && (ed.note.attachments || []).some(a => a.id === data.attachmentID)) {
        await call('attachment_replace', { id: ed.id, att: data.attachmentID, data: bytes });
        ed.app.bumpAtt(data.attachmentID);
      } else {
        const att = await call('attachment_add', { id: ed.id, filename: 'Zeichnung.png', data: bytes, type: 'public.png' });
        ed.note.attachments.push(att);
        data.attachmentID = att.id;
      }
      ed.markDirty();
      onSaved && onSaved();
    } }],
  });
}

// MARK: - Database (#22)

function databaseBlock(ed, data) {
  const conn = data.connection || { id: newId(), name: '', driver: 'sqlite', path: '', host: 'localhost', port: 0, database: '', user: '' };
  data.connection = conn;
  const field = (k, ph, w = '120px') => h('input', { type: 'text', placeholder: ph, value: conn[k] || '', style: { width: w }, oninput: e => { conn[k] = k === 'port' ? +e.target.value : e.target.value; ed.markDirty(); } });
  const drv = h('select', { onchange: e => { conn.driver = e.target.value; conn.port = { postgresql: 5432, mysql: 3306, mssql: 1433 }[conn.driver] || 0; drawConn(); ed.markDirty(); } },
    ...[['sqlite', 'SQLite'], ['postgresql', 'PostgreSQL'], ['mysql', 'MySQL / MariaDB']].map(([v, l]) => h('option', { value: v, selected: conn.driver === v }, l)));
  const connRow = h('div', { class: 'db-conn' });
  const drawConn = () => {
    connRow.innerHTML = '';
    connRow.appendChild(drv);
    if (conn.driver === 'sqlite') connRow.append(field('path', 'Pfad zur .db-Datei', '260px'), h('button', { class: 'btn', onclick: async () => { const p = await openDialog({ title: 'SQLite-Datenbank' }); if (p) { conn.path = Array.isArray(p) ? p[0] : p; drawConn(); ed.markDirty(); } } }, 'Wählen …'));
    else connRow.append(field('host', 'Host'), field('port', 'Port', '60px'), field('database', 'Datenbank'), field('user', 'Benutzer'), h('button', { class: 'btn', onclick: async () => { const pw = await prompt('Passwort', { label: 'Wird im Schlüsselbund gesichert, nicht in der Notiz.' }); if (pw !== null) password = pw; } }, 'Passwort …'));
  };
  drawConn();
  let password = null;
  const sql = h('textarea', { class: 'src-edit', spellcheck: 'false', rows: 4, placeholder: 'SELECT …' });
  sql.value = data.code || '';
  sql.addEventListener('input', () => { data.code = sql.value; ed.markDirty(); });
  sql.addEventListener('keydown', e => { if (e.key === 'Enter' && (e.metaKey || e.shiftKey)) { e.preventDefault(); run(); } });
  const result = h('div', { class: 'db-result' });
  const status = h('span', { class: 'hint' });
  const run = async () => {
    status.textContent = 'läuft …';
    try {
      const r = await call('db_query', { sql: sql.value, connection: conn, password });
      password = null;
      result.innerHTML = '';
      if (r.columns.length) {
        const t = h('table', { class: 'db-table' }, h('thead', {}, h('tr', {}, ...r.columns.map(c => h('th', {}, c)))), h('tbody', {}, ...r.rows.map(row => h('tr', {}, ...row.map(c => h('td', {}, c))))));
        result.appendChild(t);
      }
      status.textContent = r.message;
    } catch (e) { status.textContent = ''; result.innerHTML = ''; result.appendChild(h('div', { class: 'code-out err' }, String(e))); }
  };
  const el = h('div', { class: 'embed-frame' }, h('div', { class: 'embed-head' }, h('span', { class: 'title' }, 'Datenbank-Abfrage'), h('button', { class: 'primary', onclick: run }, '▶ Ausführen'), status), connRow, sql, result);
  return { el, kind: 'database', getData: () => ({ ...data, code: sql.value, language: 'sql', connection: conn }), focus: () => sql.focus() };
}

// MARK: - Embedded note (#120)

function embedBlock(ed, data) {
  const body = h('div', { class: 'embed-note' }, h('span', { class: 'hint' }, 'lädt …'));
  const title = data.code || '';
  const el = h('div', { class: 'embed-frame' }, h('div', { class: 'embed-head' }, h('span', { class: 'title' }, '↪ ' + title), h('span', { class: 'spacer' }), h('button', { onclick: () => ed.app.openWiki(title, false) }, 'Öffnen')), body);
  call('note_embed', { title }).then(r => {
    body.innerHTML = '';
    if (r.missing) { body.appendChild(h('span', { class: 'hint' }, `„${title}“ gibt es noch nicht.`)); return; }
    if (r.id === ed.id) { body.appendChild(h('span', { class: 'hint' }, 'Eine Notiz kann sich nicht selbst einbetten.')); return; }
    for (const b of r.blocks) {
      if (b.kind === 'text') { const d = h('div', { class: 'rt' }); RT.render(d, b.rich); body.appendChild(d); }
      else if (b.kind === 'code') body.appendChild(h('pre', { class: 'code-out', html: highlight(b.code || '', b.language) }));
      else if (b.kind === 'image' && b.attachmentID) body.appendChild(h('img', { class: 'media', src: attURL(r.id, b.attachmentID) }));
      else body.appendChild(h('div', { class: 'hint' }, `[${b.kind}]`));
    }
  }).catch(e => { body.textContent = String(e); });
  return { el, kind: 'embed', getData: () => ({ ...data }) };
}
