import { shortcutKey } from '../core/preferences.mjs';

export class PasteGesture {
  constructor({ paste, open, valid, delay = 450, schedule = (callback, milliseconds) => setTimeout(callback, milliseconds), unschedule = timer => clearTimeout(timer), reportError = console.error }) {
    Object.assign(this, { paste, open, valid, delay, schedule, unschedule, reportError }); this.pending = null;
  }

  down(event, context) {
    event.preventDefault();
    if (this.pending || event.repeat) return;
    const pending = { context, key: shortcutKey(event), opened: false, releaseKeys: ['CONTROL', 'META', ...(event.shiftKey ? ['SHIFT'] : []), ...(event.altKey ? ['ALT'] : [])] };
    this.pending = pending;
    try {
      pending.timer = this.schedule(() => {
        if (this.pending !== pending) return;
        try {
          if (!this.valid(pending.context)) { this.cancel(); return; }
          pending.opened = true; this.open();
        } catch (error) { this.cancel(); this.reportError(error); }
      }, this.delay);
    } catch (error) { this.pending = null; this.reportError(error); }
  }

  up(event) {
    const pending = this.pending;
    if (!pending || ![pending.key, ...pending.releaseKeys].includes(shortcutKey(event))) return false;
    event.preventDefault(); this.cancel();
    if (!pending.opened && this.valid(pending.context)) this.paste(pending.context);
    return true;
  }

  cancel() {
    const pending = this.pending; this.pending = null;
    if (pending?.timer !== undefined) {
      try { this.unschedule(pending.timer); } catch (error) { this.reportError(error); }
    }
  }
}
