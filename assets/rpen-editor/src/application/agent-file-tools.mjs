import { agentSchema, agentText, agentIndex } from '../core/agent-commands.mjs';
import { agentWorkspacePath, agentExecutableFile, agentDownloadUrl } from '../core/agent-files.mjs';
import { AgentWorkspace } from '../platform/agent-workspace.mjs';
import { t } from '../i18n.mjs';

export function registerAgentFileTools(commands, editor, workspace = new AgentWorkspace(() => editor.agentWorkspaceId)) {
  const integer = (minimum, maximum) => ({ type: 'integer', minimum, maximum });
  commands.register({ name: 'workspace_files', description: t('agent-file-tools.scd91489'), access: 'read', parameters: agentSchema({ action: { enum: ['info', 'list', 'read', 'write', 'mkdir', 'move', 'delete'] }, path: agentText, to: agentText, text: { type: 'string', maxLength: 2000000 }, base64: { type: 'string', maxLength: 3000000 }, format: { enum: ['info', 'text', 'base64'] }, overwrite: { type: 'boolean' }, recursive: { type: 'boolean' }, offset: agentIndex, limit: integer(1, 1000000) }, ['action']), prepare: async (args, { signal }) => {
    const session = editor.session, id = editor.agentWorkspaceId;
    const path = agentWorkspacePath(args.path, { root: ['info', 'list'].includes(args.action) });
    if (['info', 'list', 'read'].includes(args.action)) return { apply: () => workspace.call(args.action, { ...args, path }, signal) };
    const previous = await workspace.call('stat', { path }, signal), target = args.action === 'move' ? await workspace.call('stat', { path: agentWorkspacePath(args.to) }, signal) : null;
    if (args.action === 'write' && previous && !args.overwrite) throw Error(t('agent-file-tools.s41b076d'));
    const access = args.action === 'delete' || args.action === 'write' && (previous || agentExecutableFile(path)) || args.action === 'move' && (target || previous?.kind === 'directory' || agentExecutableFile(args.to)) ? 'dangerous' : 'edit';
    return { access, summary: t('agent-file-tools.sbc98d0f', { p0: args.action, p1: path }), details: { ...args, previous, target, undoable: false }, valid: () => editor.session === session && editor.agentWorkspaceId === id,
      apply: () => workspace.call(args.action, { ...args, path, expectedHash: previous?.hash ?? null, destinationHash: target?.hash ?? null }, signal) };
  } });
  commands.register({ name: 'workspace_download', description: t('agent-file-tools.sfd6dc68'), access: 'dangerous', parameters: agentSchema({ url: agentText, path: agentText, overwrite: { type: 'boolean' }, maxBytes: integer(1, 128 * 1024 ** 2) }, ['url', 'path']), prepare: async (args, { signal }) => {
    const url = agentDownloadUrl(args.url), path = agentWorkspacePath(args.path), session = editor.session, id = editor.agentWorkspaceId, previous = await workspace.call('stat', { path }, signal);
    if (previous && !args.overwrite) throw Error(t('agent-file-tools.s2346d6d'));
    return { summary: t('agent-file-tools.sc54d0a7') + path, details: { url, path, previous, maximumBytes: args.maxBytes ?? 128 * 1024 ** 2, undoable: false }, valid: () => editor.session === session && editor.agentWorkspaceId === id,
      apply: () => workspace.call('download', { ...args, url, path, expectedHash: previous?.hash ?? null }, signal) };
  } });
  commands.register({ name: 'workspace_import_asset', description: t('agent-file-tools.s3cf3676'), access: 'dangerous', parameters: agentSchema({ path: agentText, assetPath: agentText, overwrite: { type: 'boolean' }, binding: { enum: ['song', 'background'] } }, ['path', 'assetPath']), prepare: async (args, { signal }) => {
    const path = agentWorkspacePath(args.path), assetPath = agentWorkspacePath(args.assetPath), session = editor.session, previous = editor.assets.get(assetPath), source = await workspace.call('stat', { path }, signal);
    if (source?.kind !== 'file') throw Error(t('agent-file-tools.s0e010b4')); if (previous && !args.overwrite) throw Error(t('agent-file-tools.s707503d'));
    return { summary: t('agent-file-tools.s9f29578') + assetPath, details: { path, assetPath, bytes: source.bytes, binding: args.binding, undoable: false, needsSave: true }, valid: () => editor.session === session && editor.assets.get(assetPath) === previous,
      apply: async () => { const current = await workspace.call('stat', { path }, signal); if (current?.hash !== source.hash) throw Error(t('agent-file-tools.s025b8ae')); return editor.agent.writeAsset(assetPath, await workspace.call('bytes', { path }, signal), args.binding); } };
  } });
  return workspace;
}
