import { t } from '../i18n.mjs';
export const DEFAULT_AGENT_PROMPT = t('agent-prompt.s0f270aa') + '\r\n' + t('agent-prompt.lineDefaults');
export const migrateAgentPrompt = value => value === t('agent-prompt.s0f270aa') ? DEFAULT_AGENT_PROMPT : value;
