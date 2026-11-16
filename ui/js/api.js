// Talks to the Rust side: inside the app through Tauri, in tests through the local server.
const tauri = window.__TAURI__;
export const inApp = !!tauri;

export async function call(cmd, args = {}) {
  if (tauri) {
    return await tauri.core.invoke('call', { cmd, args });
  }
  const r = await fetch('/api', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ cmd, args }) });
  const j = await r.json();
  if ('err' in j) throw j.err;
  return j.ok;
}

/** URL of an attachment's bytes (images, PDFs, audio in the page). */
export function attURL(noteId, attId, bust = '') {
  if (tauri) return `folioatt://localhost/${noteId}/${attId}${bust ? '?v=' + bust : ''}`;
  return `/att/${noteId}/${attId}${bust ? '?v=' + bust : ''}`;
}

/** Native file dialogs (in tests: nothing is chosen). */
export async function openDialog(opts = {}) {
  if (!tauri) return window.__testDialog ? window.__testDialog(opts) : null;
  return await tauri.core.invoke('dialog_open', { opts });
}
export async function saveDialog(opts = {}) {
  if (!tauri) return window.__testDialog ? window.__testDialog(opts) : null;
  return await tauri.core.invoke('dialog_save', { opts });
}

/** Events from the native side (menu items, deep links). */
export function onNative(handler) {
  if (tauri) tauri.event.listen('folio', e => handler(e.payload));
  window.__folioNative = handler;
}

export async function native(cmd, args = {}) {
  if (!tauri) return null;
  return await tauri.core.invoke(cmd, args);
}

export async function pollJob(job, onUpdate, interval = 80) {
  for (;;) {
    const s = await call('job_poll', { job, release: false });
    onUpdate(s);
    if (s.finished) {
      await call('job_poll', { job, release: true }).catch(() => {});
      return s;
    }
    await new Promise(r => setTimeout(r, interval));
  }
}
