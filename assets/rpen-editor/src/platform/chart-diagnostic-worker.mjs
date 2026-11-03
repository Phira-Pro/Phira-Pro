import { diagnose } from '../core/chart.mjs';
import { setLocale } from '../i18n.mjs';
import { applyDiagnosticPatch } from './diagnostic-patch.mjs';

let chart;
self.onmessage = async ({ data }) => {
  const { token, options, deep } = data;
  try {
    setLocale(options.locale ?? 'zh-CN');
    chart = data.chart ?? applyDiagnosticPatch(chart, data.patch);
    if (deep) {
      const { deepDiagnostics } = await import('../core/deep-diagnostics.mjs');
      const result = await deepDiagnostics(chart, options, progress => self.postMessage({ token, progress })); self.postMessage({ token, result });
    } else self.postMessage({ token, result: diagnose(chart, options) });
  } catch (error) { self.postMessage({ token, error: error.message }); }
};
