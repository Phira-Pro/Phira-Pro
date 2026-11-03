// GPL-3.0-only. The host exposes editor storage and explicit user actions only.
(() => {
  const root = new URL('../', document.currentScript.src);
  const bytesToBase64 = bytes => {
    let text = '';
    for (let offset = 0; offset < bytes.length; offset += 16384) text += String.fromCharCode(...bytes.subarray(offset, offset + 16384));
    return btoa(text);
  };
  const fromBase64 = text => Uint8Array.from(atob(text), char => char.charCodeAt(0));
  async function request(path, body, type = 'application/json') {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), path === 'storage' || path === 'snapshot' ? 60000 : 10000);
    try {
      const response = await fetch(new URL('pro/' + path, root), { method: 'POST', headers: { 'Content-Type': type }, body, signal: controller.signal });
      const result = await response.json();
      if (!response.ok || result.ok === false) throw Object.assign(new Error(result.error?.message ?? '编辑器操作未完成'), result.error);
      return result;
    } finally { clearTimeout(timer); }
  }
  globalThis.rpenDesktopStorage = Object.freeze({ version: 1, async call(...args) {
    try {
      if (args[0] === 'binaryChunk') args[4] = bytesToBase64(args[4]);
      const response = await request('storage', JSON.stringify(args));
      if (args[0] === 'blob' && response.result?.base64 !== undefined) response.result = fromBase64(response.result.base64);
      return response;
    } catch (error) { return { ok: false, error: { name: error.name, message: error.message } }; }
  } });
  globalThis.phiraEditor = {
    root, request,
    pointerMode: 'edit',
    download(blob, name) {
      const encoded = bytesToBase64(new TextEncoder().encode(name)).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
      request('export/' + encoded, blob, blob.type || 'application/octet-stream')
        .catch(error => globalThis.rpenMods?.editor.notify(error.message, 'error'));
      return name;
    },
    requestClose() {
      if (globalThis.PhiraEditorShell) globalThis.PhiraEditorShell.closeBoot();
      return request('close', '{}');
    },
  };
  // Project modifiers onto real touch events before editor and mod listeners run.
  // Keyboard and mouse events retain their original properties.
  for(const type of ['pointerdown','pointermove','pointerup'])window.addEventListener(type,event=>{
    if(event.pointerType!=='touch')return;
    const mode=globalThis.phiraEditor.pointerMode;
    if(mode==='box')Object.defineProperty(event,'shiftKey',{value:true,configurable:true});
    if(mode==='multi')Object.defineProperty(event,'ctrlKey',{value:true,configurable:true});
  },true);
})();
