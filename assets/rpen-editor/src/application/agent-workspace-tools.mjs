import { agentSchema, agentText, agentIndex } from '../core/agent-commands.mjs';
import { beatValue } from '../core/beat.mjs';
import { t } from '../i18n.mjs';

export function registerAgentWorkspaceTools(commands, editor, { current, check, prependSilence }) {
  commands.register({ name: 'audio_view', description: t('agent-workspace-tools.s3d0ce5c'), access: 'read', parameters: agentSchema({ enabled: { type: 'boolean' }, mode: { enum: ['waveform', 'spectrum'] }, alpha: { type: 'number', minimum: .02, maximum: 1 }, width: { type: 'number', minimum: .2, maximum: 1 } }), prepare: args => {
    current(); const analysis = editor.audioAnalysis, session = editor.session; if (!analysis) throw Error(t('agent-workspace-tools.sf6d6082'));
    const changing = Object.keys(args).length > 0;
    return { access: changing ? 'edit' : 'read', summary: t('agent-workspace-tools.s74a5442'), details: args, valid: () => editor.session === session, apply: () => {
      if (changing) { Object.assign(analysis, args); analysis.onChange?.(); analysis.draw(editor.offsetSeconds?.() ?? 0); editor.invalidate?.(); }
      return { enabled: analysis.enabled, mode: analysis.mode, alpha: analysis.alpha, width: analysis.width, decoded: Boolean(analysis.data) };
    } };
  } });
  commands.register({ name: 'audio_prepend_silence', description: t('agent-workspace-tools.sce82398'), access: 'dangerous', parameters: agentSchema({ revision: agentText }, ['revision']), prepare: args => {
    const chart = check(args), session = editor.session, buffer = editor.audio.buffer, revision = editor.audio.loadRevision;
    if (!buffer || typeof prependSilence !== 'function') throw Error(t('agent-workspace-tools.s510310b'));
    const bpm = [...chart.BPMList].sort((left, right) => beatValue(left.startTime) - beatValue(right.startTime))[0]?.bpm, seconds = 240 / bpm;
    if (!Number.isFinite(seconds) || seconds <= 0) throw Error(t('agent-workspace-tools.s53a945c'));
    return { summary: t('agent-workspace-tools.sba54776'), details: { bpm, seconds, undoable: false, offsetUnchanged: true, needsSave: true }, valid: () => editor.session === session && editor.chart === chart && editor.audio.buffer === buffer && editor.audio.loadRevision === revision, apply: ({ signal }) => prependSilence({ targetSession: session, buffer, revision, seconds, signal }) };
  } });
  commands.register({ name: 'version_control', description: t('agent-workspace-tools.seb5587c'), access: 'read', parameters: agentSchema({ action: { enum: ['status', 'refs', 'log', 'diff', 'read_commit', 'initialize', 'stage', 'unstage', 'commit', 'branch_create', 'branch_rename', 'branch_delete', 'tag_create', 'tag_delete'] }, revision: agentText, name: agentText, target: agentText, commit: agentText, message: agentText, offset: agentIndex, limit: { type: 'integer', minimum: 1, maximum: 20000 } }, ['action']), prepare: async args => {
    const chart = current(), session = editor.session, panel = editor.versionControl; if (!panel) throw Error(t('agent-workspace-tools.s0dc9979'));
    const readOnly = ['status', 'refs', 'log', 'diff', 'read_commit'].includes(args.action), repo = panel.repo && panel.repoId === panel.context.getRepoId() ? panel.repo : null;
    const status = async () => { if (!repo) return { initialized: false, next: t('agent-workspace-tools.s8e08b79') }; const state = await repo.status(panel.snapshot()); return { initialized: true, head: state.headId, branch: state.branch, clean: state.clean, staged: state.staged, unstaged: state.unstaged, refs: repo.refs(state.repo) }; };
    if (readOnly) return { apply: async () => {
      if (args.action === 'status') return status();
      if (!repo) throw Error(t('agent-workspace-tools.sce5e665'));
      if (args.action === 'refs') return repo.refs(await repo.read());
      if (args.action === 'read_commit') { if (!args.commit) throw Error(t('agent-workspace-tools.s0aba140')); return repo.readCommit(args.commit); }
      if (args.action === 'log') { const entries = await repo.log(); return { total: entries.length, commits: entries.slice(args.offset ?? 0, (args.offset ?? 0) + Math.min(100, args.limit ?? 50)) }; }
      const diff = args.commit ? await repo.diffCommit(args.commit) : await repo.workingDiff(panel.snapshot()), text = JSON.stringify(diff, null, 2), offset = args.offset ?? 0, limit = args.limit ?? 12000;
      return { characters: text.length, text: text.slice(offset, offset + limit), nextOffset: offset + limit < text.length ? offset + limit : null };
    } };
    check(args); if (panel.busy) throw Error(t('agent-workspace-tools.sea7fd49'));
    if (args.action !== 'initialize' && !repo) throw Error(t('agent-workspace-tools.sce5e665'));
    if (args.action === 'commit' && !args.message?.trim()) throw Error(t('agent-workspace-tools.s5575f50'));
    if (/^(?:branch|tag)_/.test(args.action) && !args.name?.trim()) throw Error(t('agent-workspace-tools.s8f28347'));
    if (args.action === 'branch_rename' && !args.target?.trim()) throw Error(t('agent-workspace-tools.s0742a57'));
    const metadata = () => { const snapshot = panel.snapshot(); return JSON.stringify({ chartName: snapshot.chartName, folders: [...(snapshot.folders ?? [])].sort() }); };
    const before = repo ? JSON.stringify(await repo.read()) : null, assets = new Map(editor.assets), repoId = panel.context.getRepoId(), originalMetadata = metadata();
    const valid = () => editor.session === session && editor.chart === chart && !panel.busy && panel.context.getRepoId() === repoId && metadata() === originalMetadata && editor.assets.size === assets.size && [...assets].every(([name, bytes]) => editor.assets.get(name) === bytes);
    return { access: ['stage', 'unstage', 'branch_create', 'branch_rename', 'tag_create'].includes(args.action) ? 'edit' : 'dangerous', summary: t('agent-workspace-tools.s82715d8') + args.action, details: args, valid, apply: async () => {
      if (repo && JSON.stringify(await repo.read()) !== before || !valid()) throw Error(t('agent-workspace-tools.s5cdcc75'));
      panel.busy = true;
      try {
        if (args.action === 'initialize') { await panel.ensure(); await panel.actions.persistRepoId?.(); }
        else if (args.action === 'stage') await repo.stage(panel.snapshot());
        else if (args.action === 'unstage') await repo.unstage();
        else if (args.action === 'commit') await repo.commit({ message: args.message.trim() });
        else if (args.action === 'branch_create') await repo.createBranch(args.name, args.commit ?? null);
        else if (args.action === 'branch_rename') await repo.renameBranch(args.name, args.target);
        else if (args.action === 'branch_delete') await repo.deleteBranch(args.name);
        else if (args.action === 'tag_create') await repo.createTag(args.name, args.commit ?? null, args.message ?? '');
        else if (args.action === 'tag_delete') await repo.deleteTag(args.name);
        await panel.refresh(); return { applied: true, action: args.action, head: panel.state?.headId, branch: panel.state?.branch, undoable: false };
      } finally { panel.busy = false; }
    } };
  } });
}
