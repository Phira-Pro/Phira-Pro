import { t } from '../i18n.mjs';
const validate = provider => { if (typeof provider !== 'string' || !/^[a-z][a-z0-9_.-]{0,63}$/.test(provider)) throw Error(t('agent-credentials.sd01d73f')); };

export class AgentCredentials {
  constructor({ bridge = globalThis.rpenAgentCredentials, database = globalThis.indexedDB } = {}) { this.bridge = bridge?.version === 1 ? bridge : null; this.database = database; }
  async open() {
    if (!this.database) throw Error(t('agent-credentials.s4c09988'));
    return new Promise((resolve, reject) => {
      const request = this.database.open('rpe-next-agent-credentials', 1);
      request.onupgradeneeded = () => request.result.createObjectStore('keys');
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(Error(t('agent-credentials.s6df44e9')));
      request.onblocked = () => reject(Error(t('agent-credentials.s52dcf4f')));
    });
  }
  async read(provider) {
    validate(provider);
    if (this.bridge) { const response = await this.bridge.read(provider); if (!response.ok) throw Error(response.message); return response.result || ''; }
    const database = await this.open();
    try { return await new Promise((resolve, reject) => { const transaction = database.transaction('keys', 'readonly'), request = transaction.objectStore('keys').get(provider); request.onsuccess = () => resolve(typeof request.result === 'string' ? request.result : ''); transaction.onerror = () => reject(Error(t('agent-credentials.s50db176'))); transaction.onabort = () => reject(Error(t('agent-credentials.s0178841'))); }); }
    finally { database.close(); }
  }
  async write(provider, key) {
    validate(provider); if (typeof key !== 'string' || key.length > 1000 || /[\r\n]/.test(key)) throw Error(t('agent-credentials.sd85c75a'));
    if (this.bridge) { const response = await this.bridge.write(provider, key); if (!response.ok) throw Error(response.message); return; }
    if (!key && !this.database) return;
    const database = await this.open();
    try { await new Promise((resolve, reject) => { const transaction = database.transaction('keys', 'readwrite'), store = transaction.objectStore('keys'); if (key) store.put(key, provider); else store.delete(provider); transaction.oncomplete = resolve; transaction.onerror = () => reject(Error(t('agent-credentials.s51f7dd5'))); transaction.onabort = () => reject(Error(t('agent-credentials.s513bd80'))); }); }
    finally { database.close(); }
  }
}
