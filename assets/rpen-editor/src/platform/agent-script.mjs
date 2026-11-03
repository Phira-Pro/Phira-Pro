import { agentAbort, checkAgentSignal } from '../core/agent-commands.mjs';
import { t } from '../i18n.mjs';

export function runAgentChartScript(source, chart, input, signal, { timeout = 10000, workerFactory = () => new Worker(new URL('./agent-script-worker.mjs', import.meta.url), { type: 'module' }) } = {}) {
  checkAgentSignal(signal);
  return new Promise((resolve, reject) => {
    const worker = workerFactory(), token = crypto.randomUUID(); let finished = false;
    const finish = (error, value) => { if (finished) return; finished = true; clearTimeout(timer); signal?.removeEventListener('abort', abort); worker.terminate(); if (error) reject(error); else resolve(value); };
    const abort = () => finish(agentAbort()), timer = setTimeout(() => finish(Error(t('agent-script.s37e21b6'))), timeout);
    worker.onmessage = event => { if (event.data?.token === token) finish(event.data.ok ? null : Error(event.data.error), event.data); };
    worker.onerror = event => { event.preventDefault?.(); finish(Error(event.message || t('agent-script.s11b2fcc'))); };
    signal?.addEventListener('abort', abort, { once: true });
    try { worker.postMessage({ token, source, chart, input }); } catch (error) { finish(error); }
  });
}
