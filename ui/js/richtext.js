// Rich text of text blocks: the run model from Rust <-> DOM in a contenteditable, selection by offsets,
// paragraph styles, lists (plain-text markers like the Swift app), checkboxes and decorations.

export const LIST_RE = /^([\t ]*)(• |– |☐ |☑ |(\d{1,3})\. )/;
export const STYLE_SCALE = { title: 1.85, heading: 1.4, subheading: 1.15 };

let base = 15;
export function setBaseSize(px) { base = px; }
export function baseSize() { return base; }

const esc = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export function paraText(p) { return (p.runs || []).map(r => r.text).join(''); }
export function plainText(rich) { return (rich.paras || []).map(paraText).join('\n'); }

function runStyle(r) {
  const s = [];
  if (r.b) s.push('font-weight:700');
  if (r.i) s.push('font-style:italic');
  const deco = [r.u && 'underline', r.s && 'line-through'].filter(Boolean);
  if (deco.length) s.push('text-decoration:' + deco.join(' '));
  if (r.size) s.push(`font-size:${(r.size / base).toFixed(3)}em`);
  if (r.font && r.font !== 'mono') s.push(`font-family:${JSON.stringify(r.font)}, var(--body-font)`);
  if (r.color) s.push('color:' + r.color);
  if (r.bg) s.push('background-color:' + r.bg);
  return s.join(';');
}

function runHTML(r) {
  if (!r.text) return '';
  const st = runStyle(r);
  let inner = esc(r.text);
  const cls = r.font === 'mono' ? ' class="mono"' : '';
  inner = st || cls ? `<span${cls}${st ? ` style='${st}'` : ''}>${inner}</span>` : `<span>${inner}</span>`;
  if (r.link) inner = `<a href="${esc(r.link).replace(/"/g, '&quot;')}">${inner}</a>`;
  return inner;
}

export function paraHTML(p) {
  const attrs = [];
  if (p.align) attrs.push(`text-align:${p.align}`);
  if (p.indent) attrs.push(`padding-left:${p.indent}px`);
  const html = (p.runs || []).map(runHTML).join('');
  return `<div class="p"${attrs.length ? ` style="${attrs.join(';')}"` : ''}${p.align ? ` data-align="${p.align}"` : ''}${p.indent ? ` data-indent="${p.indent}"` : ''}>${html || '<br>'}</div>`;
}

export function render(root, rich) {
  const paras = rich && rich.paras && rich.paras.length ? rich.paras : [{ runs: [] }];
  root.innerHTML = paras.map(paraHTML).join('');
  decorate(root);
}

// MARK: - DOM -> model

function hex(c) {
  if (!c || c === 'transparent' || c.startsWith('rgba(0, 0, 0, 0')) return null;
  const m = c.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)(?:,\s*([\d.]+))?/);
  if (!m) return c.startsWith('#') ? c.toLowerCase() : null;
  if (m[4] !== undefined && parseFloat(m[4]) < 0.05) return null;
  return '#' + [m[1], m[2], m[3]].map(x => (+x).toString(16).padStart(2, '0')).join('');
}

const BLOCK_TAGS = new Set(['DIV', 'P', 'LI', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'BLOCKQUOTE', 'PRE', 'UL', 'OL', 'TR', 'SECTION', 'ARTICLE', 'HEADER', 'FOOTER']);

function fmtOf(el, f) {
  const n = { ...f };
  const tag = el.tagName;
  if (tag === 'B' || tag === 'STRONG') n.b = true;
  if (tag === 'I' || tag === 'EM') n.i = true;
  if (tag === 'U') n.u = true;
  if (tag === 'S' || tag === 'STRIKE' || tag === 'DEL') n.s = true;
  if (tag === 'CODE' || tag === 'TT' || tag === 'KBD' || tag === 'PRE') n.font = 'mono';
  if (tag === 'H1') { n.b = true; n.size = base * 1.85; }
  if (tag === 'H2') { n.b = true; n.size = base * 1.4; }
  if (tag === 'H3' || tag === 'H4') { n.b = true; n.size = base * 1.15; }
  if (tag === 'A' && el.getAttribute('href')) { n.link = el.getAttribute('href'); }
  if (el.classList && el.classList.contains('mono')) n.font = 'mono';
  const st = el.style;
  if (st) {
    if (st.fontWeight) n.b = st.fontWeight === 'bold' || parseInt(st.fontWeight) >= 600;
    if (st.fontStyle) n.i = st.fontStyle === 'italic' || st.fontStyle === 'oblique';
    const td = st.textDecoration || st.textDecorationLine || '';
    if (td.includes('underline')) n.u = true;
    if (td.includes('line-through')) n.s = true;
    if (st.fontSize) {
      const v = parseFloat(st.fontSize);
      if (st.fontSize.endsWith('em')) n.size = Math.round(v * (f.size || base) * 2) / 2;
      else if (st.fontSize.endsWith('px') || st.fontSize.endsWith('pt')) n.size = v;
      if (n.size && Math.abs(n.size - base) < 0.3) delete n.size;
    }
    if (st.fontFamily) {
      const ff = st.fontFamily.toLowerCase();
      if (/mono|menlo|courier|monaco/.test(ff)) n.font = 'mono';
      else if (!ff.includes('var(') && !ff.includes('system') && !ff.includes('helvetica')) n.font = st.fontFamily.split(',')[0].replace(/["']/g, '').trim();
    }
    if (st.color) n.color = hex(st.color) || undefined;
    if (st.backgroundColor) n.bg = hex(st.backgroundColor) || undefined;
  }
  if (tag === 'FONT') {
    const c = el.getAttribute('color');
    if (c) n.color = c.toLowerCase();
  }
  return n;
}

function cleanRun(text, f) {
  const r = { text };
  if (f.b) r.b = true;
  if (f.i) r.i = true;
  if (f.u && !f.link) r.u = true;
  if (f.u && f.link) r.u = true;
  if (f.s) r.s = true;
  if (f.size) r.size = f.size;
  if (f.font) r.font = f.font;
  if (f.color) r.color = f.color;
  if (f.bg) r.bg = f.bg;
  if (f.link) r.link = f.link;
  return r;
}

function same(a, b) {
  return a.b === b.b && a.i === b.i && a.u === b.u && a.s === b.s && a.size === b.size && a.font === b.font && a.color === b.color && a.bg === b.bg && a.link === b.link;
}

/** Reads the editor DOM (or any pasted HTML) into paragraphs and runs. */
export function serialize(root, { linksUnderlined = false } = {}) {
  const paras = [];
  let cur = { runs: [] };
  let curProps = {};
  const push = (text, f) => {
    if (!text) return;
    const r = cleanRun(text, f);
    if (r.link && !linksUnderlined) delete r.u;
    const last = cur.runs[cur.runs.length - 1];
    if (last && same(last, r)) last.text += text; else cur.runs.push(r);
  };
  const flush = () => {
    const p = { runs: cur.runs };
    if (curProps.align) p.align = curProps.align;
    if (curProps.indent) p.indent = curProps.indent;
    paras.push(p);
    cur = { runs: [] };
  };
  const walk = (node, f) => {
    if (node.nodeType === 3) {
      const parts = node.nodeValue.replace(/​/g, '').split('\n');
      parts.forEach((t, i) => { if (i > 0) flush(); push(t.replace(/ /g, ' '), f); });
      return;
    }
    if (node.nodeType !== 1) return;
    const tag = node.tagName;
    if (tag === 'STYLE' || tag === 'SCRIPT' || tag === 'META' || tag === 'TITLE') return;
    if (node.dataset && node.dataset.ghost) return;
    if (tag === 'BR') {
      const parent = node.parentNode;
      const isLast = !node.nextSibling || (node.nextSibling.nodeType === 3 && !node.nextSibling.nodeValue && !node.nextSibling.nextSibling);
      if (isLast && parent && BLOCK_TAGS.has(parent.tagName) && parent !== root) return;
      if (isLast && parent === root) return;
      flush();
      return;
    }
    if (tag === 'IMG') return;
    const block = BLOCK_TAGS.has(tag) && node !== root;
    const nf = fmtOf(node, f);
    let start = paras.length;
    if (block) {
      if (cur.runs.length) flush();
      start = paras.length;
      curProps = { align: node.dataset && node.dataset.align, indent: node.dataset && node.dataset.indent ? parseFloat(node.dataset.indent) : undefined };
      if (!curProps.align && node.style && node.style.textAlign && node.style.textAlign !== 'left' && node.style.textAlign !== 'start') curProps.align = node.style.textAlign;
      if (tag === 'BLOCKQUOTE') { curProps.indent = 18; nf.i = true; }
    }
    for (const c of node.childNodes) walk(c, nf);
    if (block) {
      if (cur.runs.length || paras.length === start) flush();
      curProps = {};
    }
  };
  walk(root, {});
  if (cur.runs.length || !paras.length) flush();
  return { paras };
}

// MARK: - Selection as offsets (paragraph index + offset)

function textLength(node) { return (node.textContent || '').replace(/​/g, '').length; }

function paraOf(root, node) {
  while (node && node.parentNode !== root) node = node.parentNode;
  return node;
}

function offsetIn(para, container, off) {
  const r = document.createRange();
  r.setStart(para, 0);
  try { r.setEnd(container, off); } catch { return 0; }
  return r.toString().replace(/​/g, '').length;
}

/** {start, end} as offsets into the plain text (paragraphs joined with \n), or null. */
export function getSelection(root) {
  const sel = window.getSelection();
  if (!sel.rangeCount) return null;
  const range = sel.getRangeAt(0);
  if (!root.contains(range.startContainer)) return null;
  const paras = Array.from(root.children);
  const pos = (container, off) => {
    if (container === root) {
      let g = 0;
      for (let i = 0; i < off && i < paras.length; i++) g += textLength(paras[i]) + 1;
      return Math.max(0, g - (off >= paras.length && off > 0 ? 1 : 0));
    }
    const p = paraOf(root, container);
    let g = 0;
    for (const q of paras) { if (q === p) break; g += textLength(q) + 1; }
    return g + (p ? offsetIn(p, container, off) : 0);
  };
  const start = pos(range.startContainer, range.startOffset);
  const end = range.collapsed ? start : pos(range.endContainer, range.endOffset);
  return { start: Math.min(start, end), end: Math.max(start, end) };
}

function pointAt(root, g) {
  const paras = Array.from(root.children);
  for (let i = 0; i < paras.length; i++) {
    const len = textLength(paras[i]);
    if (g <= len || i === paras.length - 1) {
      let left = Math.min(g, len);
      const walker = document.createTreeWalker(paras[i], NodeFilter.SHOW_TEXT);
      let n, last = null;
      while ((n = walker.nextNode())) {
        const l = n.nodeValue.length;
        if (left <= l) return [n, left];
        left -= l;
        last = n;
      }
      if (last) return [last, last.nodeValue.length];
      return [paras[i], 0];
    }
    g -= len + 1;
  }
  return [root, 0];
}

export function setSelection(root, start, end = start) {
  const [sn, so] = pointAt(root, start);
  const [en, eo] = pointAt(root, end);
  const r = document.createRange();
  r.setStart(sn, so);
  r.setEnd(en, eo);
  const sel = window.getSelection();
  sel.removeAllRanges();
  sel.addRange(r);
}

// MARK: - Model operations

export function splitRuns(runs, off) {
  const left = [], right = [];
  let pos = 0;
  for (const r of runs) {
    const l = r.text.length;
    if (pos + l <= off) left.push({ ...r });
    else if (pos >= off) right.push({ ...r });
    else {
      left.push({ ...r, text: r.text.slice(0, off - pos) });
      right.push({ ...r, text: r.text.slice(off - pos) });
    }
    pos += l;
  }
  return [left.filter(r => r.text), right.filter(r => r.text)];
}

/** Paragraph index and offset of a global offset. */
export function locate(rich, g) {
  for (let i = 0; i < rich.paras.length; i++) {
    const len = paraText(rich.paras[i]).length;
    if (g <= len) return [i, g];
    g -= len + 1;
  }
  const last = rich.paras.length - 1;
  return [last, paraText(rich.paras[last]).length];
}

export function globalOffset(rich, pi, off) {
  let g = 0;
  for (let i = 0; i < pi; i++) g += paraText(rich.paras[i]).length + 1;
  return g + off;
}

export function insertTextAt(p, off, text, like) {
  const [l, r] = splitRuns(p.runs, off);
  const fmt = like || l[l.length - 1] || r[0] || {};
  const run = { ...fmt, text };
  delete run.link;
  p.runs = [...l, run, ...r];
  mergeRuns(p);
}

export function deleteRange(p, from, to) {
  const [l] = splitRuns(p.runs, from);
  const [, r] = splitRuns(p.runs, to);
  p.runs = [...l, ...r];
  mergeRuns(p);
}

export function mergeRuns(p) {
  const out = [];
  for (const r of p.runs) {
    if (!r.text) continue;
    const last = out[out.length - 1];
    if (last && same(last, r)) last.text += r.text; else out.push({ ...r });
  }
  p.runs = out;
}

/** Which style a paragraph has (for the toolbar and Return). */
export function styleOf(p) {
  const runs = (p.runs || []).filter(r => r.text.trim());
  if (!runs.length) return 'body';
  const size = Math.max(...runs.map(r => r.size || base));
  const bold = runs.every(r => r.b);
  if (runs.every(r => r.font === 'mono')) return 'monospaced';
  if (size >= base * 1.6) return 'title';
  if (size >= base * 1.3) return 'heading';
  if (size >= base * 1.08 && bold) return 'subheading';
  if (p.indent && runs.every(r => r.i)) return 'quote';
  return 'body';
}

export function applyStyle(p, style) {
  const scale = STYLE_SCALE[style];
  p.runs = (p.runs || []).map(r => {
    const n = { ...r };
    delete n.size;
    if (n.font === 'mono') delete n.font;
    if (scale) { n.size = Math.round(base * scale * 2) / 2; n.b = true; }
    else if (style === 'monospaced') { n.font = 'mono'; n.b = false; }
    else if (style === 'quote') { n.i = true; n.b = false; }
    else { n.b = false; n.i = style === 'quote'; }
    return n;
  });
  if (style === 'quote') p.indent = 18; else delete p.indent;
  p._style = style;
}

export function listInfo(text) {
  const m = text.match(LIST_RE);
  if (!m) return null;
  const kind = m[2] === '• ' ? 'bullet' : m[2] === '– ' ? 'dash' : m[2].startsWith('☐') || m[2].startsWith('☑') ? 'checklist' : 'numbered';
  return { indent: m[1], marker: m[2], kind, length: m[0].length, number: m[3] ? +m[3] : null };
}

export function nextMarker(info) {
  if (info.kind === 'numbered') return `${info.number + 1}. `;
  if (info.kind === 'checklist') return '☐ ';
  return info.marker;
}

const MARKERS = { bullet: '• ', dash: '– ', checklist: '☐ ', numbered: '1. ' };

/** Turns the paragraphs into a list of `kind`, or back into text when they all are one already. */
export function toggleList(rich, from, to, kind) {
  const ps = rich.paras.slice(from, to + 1);
  const all = ps.every(p => { const i = listInfo(paraText(p)); return i && i.kind === kind; });
  let n = 1;
  for (const p of ps) {
    const t = paraText(p);
    const info = listInfo(t);
    if (info) deleteRange(p, info.indent.length, info.length);
    if (!all) {
      const indent = info ? info.indent : '';
      insertTextAt(p, indent.length, kind === 'numbered' ? `${n++}. ` : MARKERS[kind], { text: '' });
    }
  }
}

/** Indents (+1) or outdents (-1) list paragraphs with a tab. */
export function indentParas(rich, from, to, delta) {
  for (const p of rich.paras.slice(from, to + 1)) {
    const t = paraText(p);
    if (delta > 0) insertTextAt(p, 0, '\t', { text: '' });
    else if (t.startsWith('\t')) deleteRange(p, 0, 1);
    else if (t.startsWith('    ')) deleteRange(p, 0, 4);
  }
}

// MARK: - Decorations: #tags and [[links]] (spans without formatting)

const DECO_RE = /(\[\[[^\[\]\n]+?\]\])|((?:^|[^\p{L}\p{N}_#&/])#(\p{L}[\p{L}\p{N}_\-/]*))/gu;
let wikiExists = () => true;
export function setWikiResolver(fn) { wikiExists = fn; }

export function decorate(root) {
  // Remove old decorations first.
  root.querySelectorAll('span[data-deco]').forEach(s => { s.replaceWith(...s.childNodes); });
  root.normalize();
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes = [];
  let n;
  while ((n = walker.nextNode())) nodes.push(n);
  for (const node of nodes) {
    const text = node.nodeValue;
    if (!text.includes('#') && !text.includes('[[')) continue;
    if (node.parentElement && node.parentElement.closest('a')) continue;
    DECO_RE.lastIndex = 0;
    let m, last = 0;
    const frag = document.createDocumentFragment();
    let any = false;
    while ((m = DECO_RE.exec(text))) {
      let start = m.index, end = m.index + m[0].length, cls = 'wiki';
      if (m[2] !== undefined) {
        start = m.index + m[0].indexOf('#');
        let tag = m[3];
        while (/[-/]$/.test(tag)) tag = tag.slice(0, -1);
        end = start + 1 + tag.length;
        cls = 'tag';
      }
      if (start > last) frag.appendChild(document.createTextNode(text.slice(last, start)));
      const span = document.createElement('span');
      span.dataset.deco = '1';
      span.className = cls;
      const inner = text.slice(start, end);
      if (cls === 'wiki') {
        const target = inner.slice(2, -2).split('|')[0].trim();
        span.dataset.target = target;
        if (!wikiExists(target)) span.classList.add('missing');
      } else span.dataset.tag = inner.slice(1).toLowerCase();
      span.textContent = inner;
      frag.appendChild(span);
      last = end;
      any = true;
    }
    if (!any) continue;
    if (last < text.length) frag.appendChild(document.createTextNode(text.slice(last)));
    node.replaceWith(frag);
  }
}

/** Plain text before the caret in its paragraph. */
export function textBeforeCaret(root) {
  const s = getSelection(root);
  if (!s) return '';
  const rich = serialize(root);
  const [pi, off] = locate(rich, s.start);
  return paraText(rich.paras[pi]).slice(0, off);
}
