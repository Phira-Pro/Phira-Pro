// Stages mod packages as blob URLs instead of an import map.
//
// WebKit rejects a module specifier that only an import map registered after
// the first module load can resolve, which broke every mod on iOS. The staging
// below rewrites each module with the URL of its dependencies, so the entry can
// be imported directly. Cyclic graphs still need the upstream import-map flow.
import { ModRuntime as UpstreamRuntime, ModScope } from './mod-runtime.upstream.mjs';
import { stageModModules } from './mod-modules.mjs';
import { t } from '../i18n.mjs';

// The rest of the editor imports shortcutFromEvent/formatShortcut from this
// module; omitting them fails module linking before boot can show progress.
export * from './mod-runtime.upstream.mjs';

export class ModRuntime extends UpstreamRuntime {
  async prepareModules(record) {
    const loadId = record.manifest.id + '-' + crypto.randomUUID();
    const prefix = '@rpen-mod/' + loadId + '/';
    const staged = stageModModules({ files: record.files, entry: record.manifest.entry, prefix, baseUrl: this.baseUrl, loadId });
    if (!staged) return super.prepareModules(record);
    const state = {
      loadId,
      prefix,
      record,
      resolve: staged.resolve,
      urls: staged.urls,
      entry: staged.entry,
      // Upstream cleanup removes the <script type="importmap"> node; staged
      // packages never create one, so the slot stays an inert stub.
      map: { remove() {} },
    };
    this.moduleLoads.set(loadId, state);
    return state;
  }

  async activate(record, settings, onSetting) {
    const id = record.manifest.id;
    const scope = new ModScope(error => this.failure(id, error));
    const state = { id, record, scope, settings, onSetting, status: 'loading', errors: 0 };
    this.loaded.set(id, state);
    try {
      const modules = await this.prepareModules(record);
      state.modules = modules;
      scope.defer(() => { modules.urls.forEach(URL.revokeObjectURL); modules.map?.remove(); this.moduleLoads.delete(modules.loadId); });
      const api = this.createApi(state);
      state.api = api;
      // CSP allows imported package modules; packages deliberately execute with the page's authority.
      const module = await import(modules.entry ?? modules.prefix + record.manifest.entry);
      if (scope.closed) return;
      const activate = module.activate ?? module.default;
      if (typeof activate !== 'function') throw Error(t('mod-runtime.sb26e4e7'));
      const cleanup = await activate(api);
      if (typeof cleanup === 'function') scope.defer(cleanup);
      else if (cleanup?.dispose) scope.defer(() => cleanup.dispose());
      if (scope.closed) return;
      state.status = 'loaded';
      this.emit('mod:loaded', { id });
      this.changed();
      this.invalidate();
      return state;
    } catch (error) {
      scope.dispose();
      if (this.loaded.get(id) === state) this.loaded.delete(id);
      this.changed();
      if (!state.cancelled) throw error;
    }
  }
}
