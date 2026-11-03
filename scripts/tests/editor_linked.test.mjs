import test from 'node:test';
import assert from 'node:assert/strict';
import { linkedEntries, installLinkedMigration } from '../../assets/rpen-editor/pro/linked.mjs';
import { nativeAssetBytes } from '../../assets/rpen-editor/src/platform/native.mjs';
import { scanMigration, materializeProject } from '../../assets/rpen-editor/src/platform/migration.mjs';
import { createChart } from '../../assets/rpen-editor/src/core/chart.mjs';

test('linked migration materializes original media and nested names through read-only blob calls', async () => {
  const chart = createChart(); chart.META.name = 'Linked fixture'; chart.META.song = '音乐.wav';
  const entries = new Map([
    ['Resources/example/chart.json', new TextEncoder().encode(JSON.stringify(chart))],
    ['Resources/example/音乐.wav', new Uint8Array([1, 2, 3])],
  ]);
  const calls = []; const hashes = new Map([...entries].map(([name, bytes], index) => ['hash' + index, bytes]));
  const storage = { async call(command, hash, offset, length) { calls.push(command); return hashes.get(hash).slice(offset, offset + length); } };
  const input = linkedEntries({ entries: [...entries].map(([path, bytes], index) => ({ path, hash: 'hash' + index, length: bytes.length })) }, storage);
  const plan = await scanMigration(input, 'RPE');
  const project = await materializeProject(plan, plan.projects[0]);
  assert.equal(project.chart.META.name, 'Linked fixture');
  assert.deepEqual([...new Map(project.assets).get('音乐.wav')], [1, 2, 3]);
  assert.ok(calls.length > 0); assert.ok(calls.every(command => command === 'blob'));
  assert.deepEqual([...await nativeAssetBytes(await input[1].getFile())], [1, 2, 3]);
});
test('unsupported hosts keep migration unchanged and native cancellation does not select a source', async () => {
  const host = { async request() { return { result: { available: false } }; } };
  await installLinkedMigration(host); assert.equal(host.linkedMigration, undefined);
  host.request = async (_path, body) => ({ result: JSON.parse(body).action === 'available' ? { available: true } : JSON.parse(body).action === 'pick' ? { id: 'fixture' } : { cancelled: true } });
  await installLinkedMigration(host); assert.equal(await host.linkedMigration(), null);
});
