// Stages a mod package as one blob URL per module, in dependency order.
//
// WebKit only honours an import map registered before the first module load, so
// the runtime cannot rely on one: rewriting needs the real URL of every module
// it imports, and those URLs have to exist before the entry is imported. The
// graph below resolves that in two passes, first collecting edges, then
// creating the URLs dependencies first.
import { resolveModImport, rewriteModModule } from '../core/mod-package.mjs';
import { t } from '../i18n.mjs';

const decoder = new TextDecoder('utf-8', { fatal: true });
const moduleFile = /\.(m?js|json)$/;

function createModuleUrl(source) {
  return URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
}

function revoke(url) {
  URL.revokeObjectURL(url);
}

/// Returns one URL per module of `files`, or null when the module graph has a
/// cycle: a cycle needs every member's URL up front, which only an import map
/// can express. `createUrl` is injectable so the ordering can be tested without
/// a DOM.
export function stageModModules({ files, entry, prefix, baseUrl, loadId, createUrl = createModuleUrl }) {
  const sources = new Map();
  const dependencies = new Map();
  for (const [path, bytes] of files) {
    if (!moduleFile.test(path)) continue;
    if (path.endsWith('.json')) {
      sources.set(path, 'export default ' + JSON.stringify(JSON.parse(decoder.decode(bytes))) + ';');
      dependencies.set(path, []);
      continue;
    }
    const source = decoder.decode(bytes);
    const edges = new Set();
    rewriteModModule(source, path, specifier => {
      const target = resolveModImport(specifier, path, files, prefix, baseUrl);
      if (!target.startsWith(prefix)) return target;
      const dependency = target.slice(prefix.length);
      edges.add(dependency);
      return prefix + dependency; // placeholder for the recording pass
    }, loadId);
    sources.set(path, source);
    dependencies.set(path, [...edges]);
  }

  const order = moduleOrder(dependencies);
  if (!order) return null;

  const urls = new Map();
  try {
    for (const path of order) {
      const source = sources.get(path);
      const code = path.endsWith('.json') ? source : rewriteModModule(source, path, specifier => {
        const target = resolveModImport(specifier, path, files, prefix, baseUrl);
        if (!target.startsWith(prefix)) return target;
        return urls.get(target.slice(prefix.length)) ?? target;
      }, loadId);
      urls.set(path, createUrl(code));
    }
  } catch (error) {
    urls.forEach(revoke);
    throw error;
  }

  const entryUrl = urls.get(entry);
  if (!entryUrl) throw Error(t('mod-package.sb780157') + entry);
  return {
    entry: entryUrl,
    urls: [...urls.values()],
    // Same contract as resolveModImport, but relative imports return the blob
    // URL the module was staged at instead of an import-map specifier.
    resolve: (specifier, from) => {
      const target = resolveModImport(specifier, from, files, prefix, baseUrl);
      if (!target.startsWith(prefix)) return target;
      const path = target.slice(prefix.length);
      const url = urls.get(path);
      if (!url) throw Error(t('mod-package.sb780157') + path);
      return url;
    },
  };
}

/// Dependencies first, so every module can link to URLs that already exist.
/// Modules with no remaining dependency are emitted in path order to keep the
/// staged output deterministic. Returns null when the graph still has nodes.
function moduleOrder(dependencies) {
  const pending = new Map([...dependencies].map(([path, edges]) => [path, new Set(edges)]));
  const order = [];
  for (;;) {
    const ready = [...pending].filter(([, edges]) => edges.size === 0).map(([path]) => path).sort();
    if (!ready.length) break;
    for (const path of ready) {
      order.push(path);
      pending.delete(path);
    }
    for (const edges of pending.values()) for (const path of ready) edges.delete(path);
  }
  return pending.size ? null : order;
}
