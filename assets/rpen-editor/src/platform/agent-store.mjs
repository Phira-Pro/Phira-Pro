import { readLibraryRecord, writeLibraryRecord } from './library.mjs';
import { AgentConversations } from '../application/agent-conversations.mjs';
import { t } from '../i18n.mjs';
const workspaceWrites = new Map();

export class AgentConversationRepository {
  constructor(workspace, { read = readLibraryRecord, write = writeLibraryRecord } = {}) {
    if (typeof workspace !== 'string' || !workspace || workspace.length > 256) throw Error(t('agent-store.sa5d6199'));
    this.prefix = 'agent-v2:' + workspace + ':'; this.read = read; this.write = write; this.pending = Promise.resolve();
  }
  async load() {
    await (workspaceWrites.get(this.prefix) ?? this.pending).catch(() => {});
    const snapshot = await this.read(this.prefix + 'book');
    if (!snapshot || snapshot.version !== 1) return new AgentConversations();
    for (const record of snapshot.records ?? []) {
      record.turns = [];
      for (let offset = 0; offset < (record.turnIds?.length ?? 0); offset += 16) {
        const turns = await Promise.all(record.turnIds.slice(offset, offset + 16).map(id => this.read(this.prefix + 'turn:' + id)));
        record.turns.push(...turns.filter(Boolean));
      }
      if (record.memory) record.memory = await this.read(this.prefix + 'memory:' + record.id) ?? undefined;
      delete record.turnIds;
    }
    return new AgentConversations(snapshot);
  }
  save(store, { turns = [], memories = [] } = {}) {
    const snapshot = store.snapshot({ turns: false, memory: false }), changed = structuredClone(turns), compressed = memories.map(record => ({ id: record.id, memory: structuredClone(record.memory) }));
    return this.enqueue(async () => {
      for (const turn of changed) await this.write(this.prefix + 'turn:' + turn.id, turn);
      for (const record of compressed) await this.write(this.prefix + 'memory:' + record.id, record.memory);
      await this.write(this.prefix + 'book', snapshot);
    });
  }
  saveTrace(trace) { const value = structuredClone(trace); return this.enqueue(() => this.write(this.prefix + 'trace:' + value.id, value)); }
  saveTurns(turns) { const values = structuredClone(turns); return this.enqueue(async () => { for (const turn of values) await this.write(this.prefix + 'turn:' + turn.id, turn); }); }
  async trace(id) { await this.pending.catch(() => {}); return this.read(this.prefix + 'trace:' + id); }
  enqueue(action) {
    const task = (workspaceWrites.get(this.prefix) ?? this.pending).catch(() => {}).then(action); this.pending = task; workspaceWrites.set(this.prefix, task);
    const release = () => { if (workspaceWrites.get(this.prefix) === task) workspaceWrites.delete(this.prefix); }; task.then(release, release); return task;
  }
  flush() { return workspaceWrites.get(this.prefix) ?? this.pending; }
}
