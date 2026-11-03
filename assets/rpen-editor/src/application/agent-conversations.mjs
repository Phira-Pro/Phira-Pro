import { t } from '../i18n.mjs';
export const DEFAULT_AGENT_CONTEXT = Object.freeze({ mode: 'all', turns: 6 });

export function conversationMessages(conversation) {
  const context = conversation.context ?? DEFAULT_AGENT_CONTEXT;
  if (context.mode === 'none') return [];
  const available = conversation.turns.filter(turn => !['queued', 'steered', 'cancelled'].includes(turn.status));
  let turns = context.mode === 'last' ? available.slice(-Math.max(1, context.turns)) : available;
  const memory = context.mode === 'all' && conversation.memory;
  const boundary = memory ? turns.findIndex(turn => turn.id === memory.through) : -1;
  if (boundary >= 0) turns = turns.slice(boundary + 1);
  return [...(boundary >= 0 ? structuredClone(memory.messages ?? []) : []), ...turns.flatMap(turn => structuredClone(turn.messages ?? []))];
}

export class AgentConversations {
  constructor(snapshot) {
    this.records = new Map();
    for (const record of snapshot?.records ?? []) {
      if (typeof record?.id !== 'string' || !record.id || !Array.isArray(record.turns)) continue;
      const restored = structuredClone(record); restored.context ??= { ...DEFAULT_AGENT_CONTEXT }; restored.draft ??= '';
      delete restored.preparingImages;
      for (const turn of restored.turns) if (['running', 'compacting'].includes(turn.status)) { turn.status = 'stopped'; turn.error ??= t('agent-conversations.s1c12b1d'); }
      for (const turn of restored.turns) if (turn.status === 'queued') turn.paused = true;
      this.records.set(restored.id, restored);
    }
    if (!this.records.size) this.create(); else this.selected = this.records.has(snapshot.selected) ? snapshot.selected : this.records.keys().next().value;
  }
  get current() { return this.records.get(this.selected); }
  create(title = '') {
    const record = { id: crypto.randomUUID(), title: title || t('agent-conversations.s1b7abf9'), context: { ...DEFAULT_AGENT_CONTEXT }, turns: [], draft: '', createdAt: Date.now() };
    this.records.set(record.id, record); this.selected = record.id; return record;
  }
  select(id) { if (!this.records.has(id)) throw Error(t('agent-conversations.saea62e7')); this.selected = id; return this.current; }
  rename(id, title) { const record = this.records.get(id); if (!record || !title.trim()) throw Error(t('agent-conversations.s442da6f')); record.title = title.trim().slice(0, 100); }
  remove(id) { this.records.delete(id); if (!this.records.size) return this.create(); if (this.selected === id) this.selected = this.records.keys().next().value; return this.current; }
  branch(id, turnIndex) {
    const source = this.records.get(id); if (!source || !source.turns[turnIndex]) throw Error(t('agent-conversations.sa4cc469'));
    const record = this.create(source.title + t('agent-conversations.s2e72d6b')); record.context = { ...source.context };
    record.turns = structuredClone(source.turns.slice(0, turnIndex)); return record;
  }
  truncate(id, turnIndex) {
    const record = this.records.get(id); if (!record || !Number.isSafeInteger(turnIndex) || !record.turns[turnIndex]) throw Error(t('agent-conversations.sa4cc469'));
    const removed = record.turns.splice(turnIndex);
    if (record.memory && !record.turns.some(turn => turn.id === record.memory.through)) delete record.memory;
    delete record.contextUsage; this.selected = id; return removed;
  }
  snapshot({ turns = true, memory = true } = {}) {
    return { version: 1, selected: this.selected, records: [...this.records.values()].map(record => ({
      ...structuredClone(Object.fromEntries(Object.entries(record).filter(([key]) => !['turns', 'memory', 'preparingImages'].includes(key)))),
      ...(turns ? { turns: structuredClone(record.turns) } : { turnIds: record.turns.map(turn => turn.id) }),
      ...(record.memory ? { memory: memory ? structuredClone(record.memory) : { through: record.memory.through } } : {}),
    })) };
  }
  setContext(id, mode, turns = 6) {
    if (!['all', 'last', 'none'].includes(mode) || !Number.isSafeInteger(turns) || turns < 1 || turns > 10000) throw Error(t('agent-conversations.sde81c91'));
    this.records.get(id).context = { mode, turns };
  }
}
