export function scheduleSave(callback) {
  if (typeof globalThis.requestIdleCallback === 'function') globalThis.requestIdleCallback(callback, { timeout: 500 });
  else setTimeout(callback, 0);
}

export class ManualSaveQueue {
  constructor(write, schedule = scheduleSave) {
    this.write = write;
    this.schedule = schedule;
    this.pending = new Map();
  }

  save(owner, capture, complete) {
    const pending = this.pending.get(owner);
    if (pending) {
      const next = capture(), previous = pending.latest;
      const assets = next.project.assets ?? [], oldAssets = previous.project.assets ?? [];
      const folders = next.project.assetFolders ?? [], oldFolders = previous.project.assetFolders ?? [];
      if (next.document !== previous.document || assets.length !== oldAssets.length || assets.some(([name, bytes], index) => oldAssets[index]?.[0] !== name || oldAssets[index]?.[1] !== bytes)
        || folders.length !== oldFolders.length || folders.some((name, index) => oldFolders[index] !== name)) pending.next = pending.latest = next;
      return pending.promise;
    }
    const snapshot = capture();
    const entry = { latest: snapshot, next: null };
    const operation = new Promise((resolve, reject) => {
      this.schedule(async () => {
        try {
          let current = snapshot;
          while (current) {
            await this.write(current.project); complete(current);
            const next = entry.next; entry.next = null;
            if (next) {
              next.project.id = current.project.id;
              next.project.storageRevision = current.project.storageRevision;
            }
            if (!next) resolve(current.project);
            current = next;
          }
        }
        catch (error) { reject(error); }
      });
    }).finally(() => this.pending.delete(owner));
    entry.promise = operation; this.pending.set(owner, entry);
    return operation;
  }
}
