export function mediaErrorCode(error) {
  return ['AbortError', 'TimeoutError', 'TypeError', 'DataCloneError', 'SecurityError', 'OperationError', 'NotSupportedError'].includes(error?.name) ? error.name : 'Error';
}

export class MediaDiagnostics {
  constructor(emit = () => {}) { this.emit = emit; this.sequence = 0; this.active = new Map(); }
  snapshot() { return [...this.active.values()].map(entry => ({ ...entry, elapsedMs: Math.round(performance.now() - entry.started) })); }
  start(stage, details = {}) {
    const operation = ++this.sequence; const started = performance.now();
    this.active.set(operation, { operation, stage, ...details, started });
    const report = (state, extra = {}) => this.emit(`media-${stage}-${state}`, { operation, ...details, elapsedMs: Math.round(performance.now() - started), ...extra });
    report('start');
    const timer = setInterval(() => report('waiting'), 10000); timer.unref?.();
    let ended = false;
    return (state = 'complete', extra = {}) => {
      if (ended) return;
      ended = true; clearInterval(timer); this.active.delete(operation); report(state, extra);
    };
  }
}
