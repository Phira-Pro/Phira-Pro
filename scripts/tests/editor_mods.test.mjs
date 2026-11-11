import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { builtinPackageFiles } from '../../assets/rpen-editor/src/platform/builtin-packages.mjs';
import { stageModModules } from '../../assets/rpen-editor/src/ui/mod-modules.mjs';
import * as patched from '../../assets/rpen-editor/src/ui/mod-runtime.mjs';
import * as upstream from '../../assets/rpen-editor/src/ui/mod-runtime.upstream.mjs';

test('the mod loading overlay preserves every public upstream export', () => {
  for (const name of Object.keys(upstream)) {
    assert.ok(name in patched, name);
    if (name !== 'ModRuntime') assert.equal(patched[name], upstream[name], name);
  }
  assert.equal(Object.getPrototypeOf(patched.ModRuntime.prototype), upstream.ModRuntime.prototype);
});

test('the 0.14 classic layout stages its real module graph without a late import map', async t => {
  t.mock.method(globalThis, 'fetch', async url => new Response(await readFile(new URL(url))));
  const catalog = JSON.parse(await readFile(new URL('../../assets/rpen-editor/assets/mods/index.json', import.meta.url), 'utf8'));
  const entry = catalog.find(item => item.directory === 'rpe-classic-layout');
  assert.ok(entry);
  const files = await builtinPackageFiles('mods', entry);
  const manifest = JSON.parse(new TextDecoder().decode(files.get('mod.json')));
  const sources = new Map();
  const staged = stageModModules({ files, entry: manifest.entry, prefix: '@rpen-mod/fixture/',
    baseUrl: new URL('../../assets/rpen-editor/', import.meta.url).href, loadId: 'fixture',
    createUrl(source) { const url = 'blob:fixture/' + sources.size; sources.set(url, source); return url; } });
  assert.ok(staged, 'Classic layout must not fall back to a late import map');
  assert.equal(staged.entry, staged.resolve('./' + manifest.entry, ''));
  assert.ok(sources.size > 10);
  for (const source of sources.values()) assert.equal(source.includes('@rpen-mod/fixture/'), false);
});
