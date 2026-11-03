import { APP_VERSION, UPDATE_INTERVAL } from '../core/app-update.mjs';
import { UpdateChecker } from './app-update.mjs';
import { localPreferences } from './local-preferences.mjs';

const key = 'rpe-next-app-updates-v1';
export class AppUpdateClient extends EventTarget {
  constructor({ bridge = globalThis.rpenAppUpdates, fetcher = globalThis.fetch, storage = localPreferences } = {}) {
    super(); this.bridge = bridge;
    if (!bridge) {
      let state; try { state = JSON.parse(storage.getItem(key) ?? '{}'); } catch { state = {}; }
      this.checker = new UpdateChecker({ fetcher, state, persist: async value => storage.setItem(key, JSON.stringify(value)) });
    }
    this.state = { version: APP_VERSION, platform: 'web', desktop: Boolean(bridge), phase: 'idle', packages: [], autoCheck: true };
  }
  update(value) { this.state = { ...this.state, ...value }; this.dispatchEvent(new Event('change')); return this.state; }
  async call(action, value) {
    if (this.bridge) {
      const reply = await this.bridge.call(action, value); if (!reply.ok) throw Object.assign(Error(reply.error.code), { code: reply.error.code }); return reply.result;
    }
    if (action === 'check') return this.checker.check(value === true);
    if (action === 'auto') return this.checker.setAuto(value);
    return this.checker.info();
  }
  async initialize() {
    this.unsubscribe = this.bridge?.onProgress(value => this.update(value));
    this.update(await this.call('ready'));
    // One idle check on startup, then no more than daily. This has no link to
    // chart edits, playback, requestAnimationFrame, or autosave timers.
    this.startup = setTimeout(() => this.check(false), 15000);
    this.interval = setInterval(() => this.check(false), UPDATE_INTERVAL);
    this.visibility = () => { if (document.visibilityState === 'visible') this.check(false); };
    document.addEventListener('visibilitychange', this.visibility); return this;
  }
  async check(manual = true) {
    if (!manual && !this.state.autoCheck || this.state.phase !== 'idle') return this.state;
    this.update({ phase: 'checking', error: null });
    try { return this.update({ ...await this.call('check', manual), phase: 'idle' }); }
    catch (error) { return this.update({ phase: 'idle', error: error.code ?? 'UPDATE_NETWORK' }); }
  }
  async setAuto(value) { return this.update(await this.call('auto', value)); }
  async action(action, value) {
    const result = await this.call(action, value);
    this.update(await this.call('status')); return result;
  }
  dispose() { clearTimeout(this.startup); clearInterval(this.interval); document.removeEventListener('visibilitychange', this.visibility); this.unsubscribe?.(); }
}
