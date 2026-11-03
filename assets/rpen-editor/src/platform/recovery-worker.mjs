import { saveSnapshotDirect } from './recovery.mjs';
import { applySnapshotPart } from './recovery-transfer.mjs';

let snapshot; let chart;
self.onmessage = async ({ data }) => {
  try {
    if (data.kind === 'begin') snapshot = { ...data, assets: data.assets.map(([name, length]) => [name, new Uint8Array(length)]) };
    else if (data.kind === 'chart') chart = applySnapshotPart(chart, data.part);
    else if (data.kind === 'asset') snapshot.assets[data.index][1].set(data.bytes, data.offset);
    else if (data.kind === 'commit') await saveSnapshotDirect(snapshot.projectId, snapshot.name, chart, snapshot.assets, snapshot.limit, snapshot.viewState);
    self.postMessage({ ok: true });
  }
  catch (error) { self.postMessage({ ok: false, name: error.name, message: error.message }); }
};
self.postMessage({ ready: true });
