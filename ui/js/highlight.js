// Syntax highlighting for code blocks from the language table the Rust side provides (same rules as the
// Swift app's highlighter): comments, strings, numbers, keywords, builtins, types, attributes, calls.

let specs = {};
const ALIASES = { py: 'python', js: 'javascript', ts: 'typescript', sh: 'bash', shell: 'bash', rb: 'ruby', 'c++': 'cpp', golang: 'go', rs: 'rust', kt: 'kotlin', md: 'markdown', yml: 'yaml', objc: 'c', htm: 'html' };

export function setLanguages(list) {
  specs = {};
  for (const l of list) {
    specs[l.id] = {
      ...l,
      kw: new Set(l.caseInsensitive ? l.keywords.map(k => k.toLowerCase()) : l.keywords),
      bi: new Set(l.builtins),
      rules: (l.extraRules || []).map(([re, kind]) => { try { return [new RegExp(re, 'gm'), kind]; } catch { return null; } }).filter(Boolean),
    };
  }
}

export function languages() { return Object.values(specs); }
export function spec(id) { id = (id || '').toLowerCase(); return specs[id] || specs[ALIASES[id]] || null; }

const esc = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/** Token list [{from, to, kind}] for `code`. */
export function tokens(code, langId) {
  const l = spec(langId);
  if (!l) return [];
  const out = [];
  const n = code.length;
  let i = 0;
  const delims = (l.stringDelimiters || []).slice().sort((a, b) => b.length - a.length);
  const lineC = l.lineComments || [];
  const block = l.blockComment;
  const isWord = c => /[\p{L}\p{N}_]/u.test(c);
  while (i < n) {
    const c = code[i];
    if (block && code.startsWith(block[0], i)) {
      const e = code.indexOf(block[1], i + block[0].length);
      const end = e < 0 ? n : e + block[1].length;
      out.push({ from: i, to: end, kind: 'comment' });
      i = end;
      continue;
    }
    const lc = lineC.find(m => code.startsWith(m, i));
    if (lc && !(lc === '#' && l.id === 'css')) {
      const e = code.indexOf('\n', i);
      const end = e < 0 ? n : e;
      out.push({ from: i, to: end, kind: 'comment' });
      i = end;
      continue;
    }
    const d = delims.find(m => code.startsWith(m, i));
    if (d) {
      let j = i + d.length;
      while (j < n) {
        if (code[j] === '\\' && d.length === 1) { j += 2; continue; }
        if (code.startsWith(d, j)) { j += d.length; break; }
        if (code[j] === '\n' && d.length === 1 && d !== '`') break;
        j++;
      }
      out.push({ from: i, to: Math.min(j, n), kind: 'string' });
      i = Math.min(j, n);
      continue;
    }
    if (/[0-9]/.test(c) && (i === 0 || !isWord(code[i - 1]))) {
      let j = i + 1;
      while (j < n && /[0-9a-fA-FxXoObB_.eE]/.test(code[j])) j++;
      out.push({ from: i, to: j, kind: 'number' });
      i = j;
      continue;
    }
    if (l.attributePrefix && c === l.attributePrefix) {
      let j = i + 1;
      while (j < n && (isWord(code[j]) || code[j] === '.' || code[j] === '{' && j === i + 1)) j++;
      if (j > i + 1) { out.push({ from: i, to: j, kind: 'attribute' }); i = j; continue; }
    }
    if (isWord(c) && (i === 0 || !isWord(code[i - 1]))) {
      let j = i + 1;
      while (j < n && isWord(code[j])) j++;
      if (code[j] === '?' && l.id === 'ruby') j++;
      const w = code.slice(i, j);
      const key = l.caseInsensitive ? w.toLowerCase() : w;
      let kind = null;
      if (l.kw.has(key)) kind = 'keyword';
      else if (l.bi.has(w)) kind = 'type';
      else if (l.capitalizedAreTypes && /^[A-Z]/.test(w) && /[a-z]/.test(w)) kind = 'type';
      else if (code[j] === '(') kind = 'function';
      if (kind) out.push({ from: i, to: j, kind });
      i = j;
      continue;
    }
    i++;
  }
  // Extra rules fill gaps that are not strings or comments.
  if (l.rules.length) {
    const taken = out.filter(t => t.kind === 'string' || t.kind === 'comment');
    for (const [re, kind] of l.rules) {
      re.lastIndex = 0;
      let m;
      while ((m = re.exec(code))) {
        if (!m[0].length) { re.lastIndex++; continue; }
        const from = m.index, to = m.index + m[0].length;
        if (taken.some(t => from < t.to && t.from < to)) continue;
        for (let k = out.length - 1; k >= 0; k--) if (out[k].from >= from && out[k].to <= to && out[k].kind !== 'string' && out[k].kind !== 'comment') out.splice(k, 1);
        out.push({ from, to, kind });
      }
    }
    out.sort((a, b) => a.from - b.from);
  }
  return out;
}

export function highlight(code, langId) {
  const toks = tokens(code, langId);
  let html = '', pos = 0;
  for (const t of toks) {
    if (t.from < pos) continue;
    html += esc(code.slice(pos, t.from)) + `<span class="tok-${t.kind}">${esc(code.slice(t.from, t.to))}</span>`;
    pos = t.to;
  }
  html += esc(code.slice(pos));
  // A trailing newline needs a character after it to get its own line height.
  return html + (code.endsWith('\n') || !code ? ' ' : '');
}
