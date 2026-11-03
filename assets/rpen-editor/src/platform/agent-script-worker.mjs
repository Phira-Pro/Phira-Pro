import { createNote, createEvent, createLine } from '../core/chart.mjs';
import { createNoiseArea, createNoiseEvent } from '../core/noise-areas.mjs';
import { fromNumber, beatValue, parseBeat } from '../core/beat.mjs';
import { easing } from '../core/easing.mjs';
import { t } from '../i18n.mjs';

const send = globalThis.postMessage.bind(globalThis);
globalThis.onmessage = async event => {
  const { token, source, chart, input } = event.data;
  const origins = new WeakMap(chart.judgeLineList.map((line, index) => [line, index]));
  for (const name of ['fetch', 'XMLHttpRequest', 'WebSocket', 'EventSource', 'Worker', 'SharedWorker', 'importScripts', 'indexedDB', 'caches', 'BroadcastChannel', 'navigator', 'postMessage', 'close', 'event']) {
    for (let scope = globalThis; scope; scope = Object.getPrototypeOf(scope)) if (scope === globalThis || Object.hasOwn(scope, name)) Object.defineProperty(scope, name, { value: undefined, configurable: false, writable: false });
  }
  let url;
  try {
    const logs = [], api = Object.freeze({ createNote, createEvent, createLine, createNoiseArea, createNoiseEvent, fromNumber, beatValue, parseBeat, easing, clone: structuredClone.bind(globalThis), report: value => logs.push(value) });
    url = URL.createObjectURL(new Blob(['export default async function(chart,api,input){"use strict";\n' + source + '\n}'], { type: 'text/javascript' }));
    const script = await import(url), result = await script.default(chart, api, input);
    const next = result?.chart ?? chart;
    if (JSON.stringify(next).length > 128 * 1024 * 1024) throw Error(t('agent-script-worker.s895ce54'));
    const output = result?.result ?? result ?? logs;
    send({ token, ok: true, chart: next, origins: next.judgeLineList.map(line => origins.get(line) ?? null), result: JSON.stringify(output).length > 60000 ? { message: t('agent-script-worker.s08b8013') } : output });
  } catch (error) { send({ token, ok: false, error: error.message }); }
  finally { if (url) URL.revokeObjectURL(url); }
};
