// GPL-3.0-only. Copy-only native library actions, isolated from RPEN persistence.
import { createChartExport, resourceReferences, assetPath } from '../src/platform/files.mjs';

export async function libraryOperation(host, request, { sleep = ms => new Promise(resolve => setTimeout(resolve, ms)), now = Date.now } = {}) {
  const { id } = await host.request('library', JSON.stringify(request));
  const deadline = now() + 120000;
  while (now() < deadline) {
    const { result } = await host.request(`library-status/${id}`, '{}');
    if (result.state === 'failed') throw Error(result.message);
    if (result.state === 'ready') return { id, ...result };
    await sleep(150);
  }
  throw Error('谱面库操作超时，请先返回 Pro 检查谱面库，再决定是否重试');
}

export function copyExport(editor) {
  const refs = resourceReferences(editor.chart, editor.assets, editor.chartName);
  if (!refs.song || !assetPath(editor.assets, refs.song, editor.chartName)) throw Error('存入 Pro 谱面库前，请先导入并指定谱面音乐');
  // Discard stale native metadata in the exported copy only. RPEN generates info.txt
  // from the current chart and resource bindings; the source assets remain intact.
  const assets = new Map([...editor.assets].filter(([name]) => !/^(?:info\.yml|info\.csv)$/i.test(name)));
  const chart = { ...editor.chart, META: { ...editor.chart.META, useRpen: true } };
  return createChartExport(chart, assets, editor.chartName, { format: 'pez', compatibility: 'next', includeUnusedFiles: true }).blob;
}

export function installLibrary({ editor, host, action, save, isBusy, setBusy, status, report }) {
  const dialog = document.createElement('dialog');
  dialog.id = 'pro-library'; dialog.className = 'pro-dialog pro-library'; dialog.dataset.modInteractive = 'true';
  dialog.innerHTML = '<h2>从 Pro 谱面库导入</h2><p class="pro-warning">导入独立编辑副本，原谱面和成绩保持不变。</p><input class="pro-library-search" type="search" placeholder="搜索曲名、难度或作者" aria-label="搜索 Pro 谱面库"><p class="pro-library-message" role="status"></p><div class="pro-library-list"></div><div class="pro-actions"><button type="button" data-cancel>取消</button></div>';
  document.body.append(dialog);
  const list = dialog.querySelector('.pro-library-list'), search = dialog.querySelector('input'), message = dialog.querySelector('[role=status]'), cancel = dialog.querySelector('[data-cancel]');
  let rows = [], working = false, choose;
  const lock = value => { working = value; setBusy(value); search.disabled = value; cancel.disabled = value; list.querySelectorAll('button').forEach(button => { button.disabled = value; }); };
  dialog.addEventListener('cancel', event => { if (working) event.preventDefault(); });
  dialog.addEventListener('close', () => { choose?.(null); choose = null; });
  cancel.onclick = () => dialog.close();
  function render() {
    list.replaceChildren();
    const query = search.value.trim().toLocaleLowerCase();
    for (const chart of rows.filter(chart => [chart.name, chart.level, chart.charter, chart.composer].join(' ').toLocaleLowerCase().includes(query))) {
      const button = document.createElement('button'); button.type = 'button'; button.className = 'pro-library-chart';
      const title = document.createElement('strong'); title.textContent = chart.name || '未命名谱面';
      const detail = document.createElement('span'); detail.textContent = [chart.level, chart.charter, chart.composer].filter(Boolean).join(' / ');
      button.append(title, detail); button.disabled = working; button.onclick = () => read(chart); list.append(button);
    }
    message.textContent = list.childElementCount ? `共 ${list.childElementCount} 个谱面` : rows.length ? '没有匹配的谱面' : 'Pro 本地谱面库暂无谱面';
  }
  search.oninput = render;
  const selectCandidate = candidates => new Promise(resolve => {
    choose = resolve; lock(false); search.hidden = true; list.replaceChildren(); message.textContent = '此包包含多个谱面，请选择要编辑的一份';
    candidates.forEach((candidate, index) => {
      const button = document.createElement('button'); button.type = 'button'; button.textContent = [candidate.name, candidate.title].filter(Boolean).join(' / ');
      button.onclick = () => { choose = null; lock(true); resolve(index); }; list.append(button);
    });
  });
  async function read(chart) {
    if (working) return; lock(true); message.textContent = '正在读取谱面副本';
    try {
      await save();
      const result = await libraryOperation(host, { action: 'read', key: chart.key });
      const response = await fetch(new URL(`pro/library-package/${result.id}`, host.root), { signal: AbortSignal.timeout(60000) });
      if (!response.ok) throw Error('谱面副本读取失败，请重新打开列表');
      const file = new File([await response.blob()], 'chart.pez', { type: 'application/zip' });
      const imported = await editor.importFiles([file], { selectCandidate, metadata: { name: chart.name, level: chart.level, charter: chart.charter, composer: chart.composer } });
      if (imported) { await save(); dialog.close(); status.textContent = '已导入独立编辑副本'; }
    } catch (error) { message.textContent = error.message; report(error); }
    finally { lock(false); }
  }
  const importButton = action('从 Pro 导入', async () => {
    if (isBusy()) return;
    search.hidden = false; search.value = ''; rows = []; list.replaceChildren(); dialog.showModal(); lock(true); message.textContent = '正在读取 Pro 谱面库';
    try { rows = (await libraryOperation(host, { action: 'list' })).value.charts; lock(false); render(); }
    catch (error) { lock(false); message.textContent = error.message; }
  });
  importButton.id = 'pro-library-import';
  const exportButton = action('存入 Pro 谱面库', async () => {
    if (isBusy()) return;
    if (!editor.hasDocument) throw Error('请先新建或打开一个工程');
    lock(true);
    try {
      editor.pause(); editor.noiseEditor.flush(); await save();
      status.textContent = '正在创建 Pro 谱面副本';
      const snapshot = await host.request('snapshot', copyExport(editor), 'application/zip');
      const result = await libraryOperation(host, { action: 'write', snapshot: snapshot.id });
      status.textContent = `已存入 Pro 谱面库：${result.value.name}`; editor.notify('已创建新谱面，原谱面未被覆盖', 'success');
    } finally { lock(false); }
  });
  exportButton.id = 'pro-library-export';
  return [importButton, exportButton];
}
