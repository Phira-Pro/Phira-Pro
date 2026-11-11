import test from 'node:test';
import assert from 'node:assert/strict';
import { libraryOperation, copyExport } from '../../assets/rpen-editor/pro/library.mjs';
import { createChart, createNote } from '../../assets/rpen-editor/src/core/chart.mjs';
import { readZip } from '../../assets/rpen-editor/src/platform/archive.mjs';

test('library handoff waits for completion and surfaces failure without retrying writes', async () => {
  const calls = []; let clock = 0;
  const host = { async request(path, body) {
    calls.push([path, body]);
    if (path === 'library') return { id: 'operation' };
    return { result: calls.length < 3 ? { state: 'pending' } : { state: 'ready', kind: 'json', value: { created: true } } };
  } };
  const result = await libraryOperation(host, { action: 'write', snapshot: 'fixture' }, { now: () => clock, sleep: async ms => { clock += ms; } });
  assert.equal(result.value.created, true);
  assert.equal(calls.filter(([path]) => path === 'library').length, 1);
  host.request = async path => path === 'library' ? { id: 'failure' } : { result: { state: 'failed', message: 'Fixture failure' } };
  await assert.rejects(libraryOperation(host, { action: 'list' }), /Fixture failure/);
  host.request = async path => path === 'library' ? { id: 'pending' } : { result: { state: 'pending' } };
  await assert.rejects(libraryOperation(host, { action: 'list' }, { now: () => clock, sleep: async () => { clock += 120000; } }), /超时/);
});

test('copy export uses edited chart and fresh metadata without mutating the imported source', async () => {
  const chart = createChart(); chart.META.name = 'Edited fixture'; chart.META.song = 'music.wav';
  chart.judgeLineList[0].notes = [createNote(1, 1, 0)];
  const encode = value => new TextEncoder().encode(value);
  const assets = new Map([['info.yml', encode('chart: stale.json\nname: Original\n')], ['info.txt', encode('Name: Original\n')], ['music.wav', new Uint8Array([1, 2, 3])], ['chart.json', encode('{}')]]);
  const source = [...assets].map(([name, bytes]) => [name, [...bytes]]), before = JSON.stringify(chart);
  const blob = await copyExport({ chart, assets, chartName: 'chart.json' });
  const output = await readZip(await blob.arrayBuffer());
  assert.equal(output.has('info.yml'), false);
  assert.match(new TextDecoder().decode(output.get('info.txt')), /Name: Edited fixture/);
  const saved = JSON.parse(new TextDecoder().decode(output.get('chart.json')));
  assert.equal(saved.judgeLineList[0].notes.length, 1);
  assert.equal(saved.META.useRpen, true);
  assert.deepEqual([...assets].map(([name, bytes]) => [name, [...bytes]]), source);
  assert.equal(JSON.stringify(chart), before);
  await assert.rejects(copyExport({ chart, assets: new Map(), chartName: 'chart.json' }), /音乐/);
});
