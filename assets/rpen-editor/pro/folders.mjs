// Android's document tree is transported as a temporary archive. Keep the
// original directory input and its change handler, including extension inputs.
import { readZip } from '../src/platform/archive.mjs';

export const FOLDER_TYPE = 'application/vnd.phira.rpen-folder+zip';
export const FOLDER_LIMITS = { maxFiles: 20000, maxBytes: 512 * 1024 ** 2, maxFileBytes: 512 * 1024 ** 2 };

export async function directoryFiles(buffer) {
  const entries = await readZip(buffer, FOLDER_LIMITS);
  const files = [];
  for (const [path, bytes] of entries) {
    if (path.endsWith('/')) continue;
    const parts = path.split('/');
    if (parts.length < 2 || parts.some(part => !part || part === '.')) throw Error('目录中的文件路径无效');
    const file = new File([bytes], parts.at(-1));
    Object.defineProperty(file, 'webkitRelativePath', { value: path });
    files.push(file);
  }
  if (!files.length) throw Error('所选文件夹中没有文件');
  return files;
}

export function installFolders(report) {
  if (!globalThis.PhiraEditorShell?.directoryPicker) return;
  let busy = false;
  const pick = token => new Promise((resolve, reject) => {
    if (busy) { reject(new DOMException('文件夹选择器正在使用', 'InvalidStateError')); return; }
    busy = true;
    const picker = document.createElement('input');
    picker.type = 'file'; picker.accept = token ? `application/vnd.phira.rpen-folder-${token}+zip` : FOLDER_TYPE; picker.hidden = true;
    const clean = () => { picker.remove(); busy = false; };
    picker.addEventListener('cancel', () => { clean(); reject(new DOMException('已取消选择', 'AbortError')); }, { once: true });
    picker.addEventListener('change', async () => {
      try {
        if (!picker.files.length) throw new DOMException('已取消选择', 'AbortError');
        const file = picker.files[0];
        resolve({ files: await directoryFiles(await file.arrayBuffer()), token: file.name.replace(/\.zip$/, '') });
      } catch (error) { reject(error); }
      finally { clean(); }
    }, { once: true });
    document.body.append(picker);
    picker.click();
  });
  // Some WebViews expose the desktop API but abort every request. Supply its
  // read-only surface too, so skins and migration keep their original flows.
  window.showDirectoryPicker = async () => {
    let selection = await pick();
    return directoryHandle(selection.files, async () => {
      selection = await pick(selection.token);
      return selection.files;
    });
  };
  document.addEventListener('click', event => {
    const original = event.target;
    if (!(original instanceof HTMLInputElement) || original.type !== 'file' || !original.webkitdirectory) return;
    event.preventDefault(); event.stopImmediatePropagation();
    if (busy || original.disabled) return;
    pick().then(({ files }) => {
      const transfer = new DataTransfer();
      for (const file of files) transfer.items.add(file);
      original.files = transfer.files;
      original.dispatchEvent(new Event('change', { bubbles: true }));
    }).catch(error => { if (error.name !== 'AbortError') report(error); });
  }, true);
}

export function directoryHandle(files, refresh = null) {
  const root = branch(files[0].webkitRelativePath.split('/')[0]);
  for (const file of files) {
    const parts = file.webkitRelativePath.split('/');
    if (parts.shift() !== root.name) throw Error('文件夹根目录不一致');
    let parent = root;
    for (const name of parts.slice(0, -1)) {
      if (!parent.children.has(name)) parent.children.set(name, branch(name));
      parent = parent.children.get(name);
    }
    parent.children.set(file.name, { kind: 'file', name: file.name, getFile: async () => new File([file], file.name, { type: file.type, lastModified: file.lastModified }) });
  }
  if (refresh) {
    let firstRead = true;
    const current = async () => {
      if (firstRead) { firstRead = false; return root; }
      return directoryHandle(await refresh());
    };
    root.values = async function* () { yield* (await current()).children.values(); };
    root.entries = async function* () { yield* (await current()).children.entries(); };
  }
  return root;
}

function branch(name) {
  const children = new Map();
  return { kind: 'directory', name, children, values: () => children.values(), entries: () => children.entries() };
}
