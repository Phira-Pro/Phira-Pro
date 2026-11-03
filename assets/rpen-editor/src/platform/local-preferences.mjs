import { nativeStorage, nativeReady, nativeWrite } from './native.mjs';

const values = new Map();
const pending = new Set();
const dirty = new Map();

export async function initializeLocalPreferences() {
  if (!nativeStorage) return;
  await nativeReady();
  const record = await nativeStorage.call('read', 'preferences', 'ui-settings');
  if (record) {
    for (const [key, value] of Object.entries(record.data.values ?? {})) if (typeof value === 'string') values.set(key, value);
  } else {
    await nativeWrite('preferences', 'ui-settings', { id: 'ui-settings', values: Object.fromEntries(values) });
  }
}

function update(key, value) {
  if (value === null) values.delete(key); else values.set(key, value);
  const entry = { value }; dirty.set(key, entry);
  send(key, entry).catch(() => {});
}
function send(key, entry) {
  const task = nativeStorage.call('setLocalPreference', key, entry.value);
  pending.add(task);
  task.then(() => {
    if (dirty.get(key) === entry) dirty.delete(key);
  }, error => {
    document.dispatchEvent(new CustomEvent('rpen-preference-error', { detail: error }));
  }).finally(() => pending.delete(task));
  return task;
}

// Reads stay synchronous for the UI; only the storage worker writes to disk.
export const localPreferences = {
  getItem(key) { return nativeStorage ? values.get(String(key)) ?? null : globalThis.localStorage?.getItem(key) ?? null; },
  setItem(key, value) { if (nativeStorage) update(String(key), String(value)); else globalThis.localStorage?.setItem(key, value); },
  removeItem(key) { if (nativeStorage) update(String(key), null); else globalThis.localStorage?.removeItem(key); },
};
export async function flushLocalPreferences() {
  while (pending.size) await Promise.allSettled([...pending]);
  for (const [key, entry] of [...dirty]) await send(key, entry);
  while (pending.size) await Promise.allSettled([...pending]);
}
