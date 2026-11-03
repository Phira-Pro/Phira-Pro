// GPL-3.0-only. Native directory access stays behind an explicit user selection.
import { nativeStorage, decodeNative } from '../src/platform/native.mjs';

export function linkedEntries(selection, storage = nativeStorage) {
  return selection.entries.map(entry => ({
    path: entry.path,
    async getFile() {
      // Reuse the storage decoder so saves and recovery retain the source hash.
      const { file } = await decodeNative({ data: { file: null }, binaries: [
        { path: ['file'], hash: entry.hash, length: entry.length, kind: 'blob', type: '' },
      ] }, storage);
      return file;
    },
  }));
}
export async function installLinkedMigration(host) {
  const call = async body => (await host.request('linked', JSON.stringify(body))).result;
  if (!(await call({ action: 'available' })).available) return;
  host.linkedMigration = async () => {
    const { id } = await call({ action: 'pick' });
    const deadline = Date.now() + 10 * 60 * 1000;
    while (Date.now() < deadline) {
      const result = await call({ action: 'status', id });
      if (result.cancelled) return null;
      if (result.error) throw Error(result.error);
      if (result.selection) return { name: result.selection.name, entries: linkedEntries(result.selection) };
      await new Promise(resolve => setTimeout(resolve, 250));
    }
    throw Error('目录选择尚未完成，请先关闭目录选择窗口');
  };
}
