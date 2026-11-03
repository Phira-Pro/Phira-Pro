export class AutoSaveClock {
  constructor(save, reportError) {
    this.save = save;
    this.reportError = reportError;
    this.last = 0;
    this.pending = false;
    this.timer = null;
  }
  reset(now) { this.last = now; }
  tick(now, enabled, seconds, dirty) {
    if (!enabled || !dirty || this.pending || now - this.last < seconds * 1000) return;
    this.last = now; this.pending = true;
    let resolveSave;
    const finished = new Promise(resolve => { resolveSave = resolve; });
    const save = async () => {
      try { await this.save(); } catch (error) { this.reportError(error); }
      finally { this.pending = false; this.timer = null; resolveSave(); }
    };
    if (typeof globalThis.requestIdleCallback === 'function') {
      this.timer = globalThis.requestIdleCallback(save, { timeout: 1000 });
    } else {
      this.timer = globalThis.setTimeout(save, 0);
    }
    return finished;
  }
}
