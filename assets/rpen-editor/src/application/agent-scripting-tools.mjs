import { agentSchema, agentText } from '../core/agent-commands.mjs';
import { agentScriptChanges } from '../core/agent-script.mjs';
import { runAgentChartScript } from '../platform/agent-script.mjs';
import { t } from '../i18n.mjs';

export function registerAgentScriptingTools(commands, editor, { check, prepareChart, script = runAgentChartScript }) {
  commands.register({ name: 'agent_tools', description: t('agent-scripting-tools.s1172134'), access: 'read', parameters: agentSchema({ query: agentText, names: { type: 'array', items: agentText, maxItems: 30 }, limit: { type: 'integer', minimum: 1, maximum: 30 } }), prepare: args => ({ apply: () => {
    const entries = [...commands.entries.values()];
    if (!args.query && !args.names?.length) return { categories: [...new Set(entries.map(entry => entry.name.split('_')[0]))], total: entries.length, hint: t('agent-scripting-tools.sb6ae55f') };
    const query = (args.query ?? '').toLowerCase();
    const selected = entries.filter(entry => args.names?.includes(entry.name) || query && (entry.name + ' ' + entry.description).toLowerCase().includes(query));
    return { total: selected.length, commands: selected.slice(0, args.limit ?? 20).map(entry => ({ name: entry.name, description: entry.description, access: entry.access, owner: entry.owner })) };
  } }) });
  commands.register({ name: 'chart_script', description: t('agent-scripting-tools.sa808291'), access: 'edit', parameters: agentSchema({ revision: agentText, source: { type: 'string', maxLength: 200000 }, input: {}, label: agentText, dryRun: { type: 'boolean' }, timeoutMs: { type: 'integer', minimum: 100, maximum: 30000 } }, ['revision', 'source']), prepare: async (args, { signal }) => {
    const before = check(args), result = await script(args.source, before, args.input, signal, { timeout: args.timeoutMs ?? 10000 });
    const changes = agentScriptChanges(before, result.chart, result.origins);
    const plan = prepareChart(args, before, result.chart, args.label ?? t('agent-scripting-tools.s272c828'), { changes, result: result.result }, changes.dangerous);
    plan.details = { ...plan.details, source: args.source, input: args.input };
    if (args.dryRun) return { ...plan, access: 'read', apply: () => ({ dryRun: true, ...changes, result: result.result }) };
    return plan;
  } });
}
