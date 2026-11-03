import { t } from '../i18n.mjs';
export const AGENT_API_VERSION = 1;
export const AGENT_PERMISSIONS = ['ask', 'dangerous', 'full'];
export const agentAbort = () => new DOMException(t('agent-commands.sb5b228a'), 'AbortError');
export function checkAgentSignal(signal) { if (signal?.aborted) throw agentAbort(); }

export function validateAgentValue(value, schema = {}, path = 'arguments', depth = 0) {
  if (depth > 40) throw Error(t('agent-commands.s13f8be2'));
  if (schema.anyOf) {
    let matched = false;
    for (const option of schema.anyOf) { try { validateAgentValue(value, option, path, depth + 1); matched = true; break; } catch {} }
    if (!matched) throw Error(path + t('agent-commands.s388f69b'));
  }
  if (schema.enum && !schema.enum.some(option => JSON.stringify(option) === JSON.stringify(value))) throw Error(path + t('agent-commands.sd1695e8'));
  if (schema.type === 'object') {
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw Error(path + t('agent-commands.s4978249'));
    for (const key of schema.required ?? []) if (!Object.hasOwn(value, key)) throw Error(path + '.' + key + t('agent-commands.sad41257'));
    for (const [key, entry] of Object.entries(value)) {
      const property = Object.hasOwn(schema.properties ?? {}, key) ? schema.properties[key] : null;
      if (!property && schema.additionalProperties === false) throw Error(path + '.' + key + t('agent-commands.s2aa8e1c'));
      validateAgentValue(entry, property ?? {}, path + '.' + key, depth + 1);
    }
  } else if (schema.type === 'array') {
    if (!Array.isArray(value) || value.length > (schema.maxItems ?? 10000) || value.length < (schema.minItems ?? 0)) throw Error(path + t('agent-commands.s01cf906'));
    value.forEach((entry, index) => validateAgentValue(entry, schema.items, path + '[' + index + ']', depth + 1));
  } else if (schema.type === 'number' || schema.type === 'integer') {
    if (!Number.isFinite(value) || schema.type === 'integer' && !Number.isSafeInteger(value) || value < (schema.minimum ?? -Infinity) || value > (schema.maximum ?? Infinity)) throw Error(path + t('agent-commands.s64c7510'));
  } else if (schema.type === 'string') {
    if (typeof value !== 'string' || value.length > (schema.maxLength ?? 100000)) throw Error(path + t('agent-commands.s6f488df'));
  } else if (schema.type === 'boolean' && typeof value !== 'boolean') throw Error(path + t('agent-commands.sdb758e5'));
}

export class AgentCommands extends EventTarget {
  constructor() { super(); this.entries = new Map(); this.running = false; }
  register(definition, owner = 'rpen') {
    if (!/^[a-z][a-z0-9_]{0,63}$/.test(definition.name) || this.entries.has(definition.name)) throw Error(t('agent-commands.s5c5ad58'));
    if (!['read', 'edit', 'dangerous'].includes(definition.access) || typeof definition.prepare !== 'function') throw Error(t('agent-commands.s909da8a'));
    const entry = Object.freeze({ ...definition, owner }); this.entries.set(entry.name, entry); this.dispatchEvent(new Event('change'));
    return () => { if (this.entries.get(entry.name) === entry) { this.entries.delete(entry.name); this.dispatchEvent(new Event('change')); } };
  }
  tools() { return [...this.entries.values()].map(entry => ({ type: 'function', function: { name: entry.name, description: entry.description, parameters: entry.parameters } })); }
  async execute(name, args, { permission = 'ask', approve, signal, guard = () => true } = {}) {
    if (this.running) throw Error(t('agent-commands.s5c4c83f'));
    if (!AGENT_PERMISSIONS.includes(permission)) throw Error(t('agent-commands.s8614dbf'));
    const entry = this.entries.get(name); if (!entry) throw Error(t('agent-commands.s18693b3') + name);
    validateAgentValue(args, entry.parameters); checkAgentSignal(signal); this.running = true;
    try {
      const prepared = await entry.prepare(structuredClone(args), { signal }); checkAgentSignal(signal);
      const access = entry.access === 'dangerous' || prepared.access === 'dangerous' ? 'dangerous' : entry.access === 'read' && prepared.access === 'edit' ? 'edit' : prepared.access === 'read' ? 'read' : entry.access;
      const plan = { command: name, owner: entry.owner, access, summary: prepared.summary ?? entry.description, details: prepared.details ?? args };
      if (access !== 'read' && (permission === 'ask' || permission === 'dangerous' && access === 'dangerous')) {
        if (!approve || !await approve(plan, signal)) return { ok: false, denied: true, message: t('agent-commands.s44d215d') };
      }
      checkAgentSignal(signal);
      if (!guard() || prepared.valid && !prepared.valid() || this.entries.get(name) !== entry) throw Error(t('agent-commands.sba8a65d'));
      const result = await prepared.apply({ signal, permission }); checkAgentSignal(signal);
      return { ok: true, result: result ?? null };
    } finally { this.running = false; }
  }
}

export const agentSchema = (properties = {}, required = []) => ({ type: 'object', properties, required, additionalProperties: false });
export const agentNumber = { type: 'number' };
export const agentText = { type: 'string' };
export const agentIndex = { type: 'integer', minimum: 0 };
