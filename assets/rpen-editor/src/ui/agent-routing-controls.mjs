import { agentEffortLabel, agentEffortDefault } from '../core/agent-routing.mjs';
import { t } from '../i18n.mjs';

const text = (tag, value, className = '') => { const node = document.createElement(tag); node.textContent = value; node.className = className; return node; };
const option = (value, label) => { const node = text('option', label); node.value = value; return node; };
export const agentModelLabel = model => String(model ?? '').split('/').at(-1).replace(/^~/, '');

export class AgentRoutingControls {
  constructor(panel) {
    this.panel = panel;
    this.toggle = text('button', '', 'agent-route-toggle'); this.toggle.type = 'button'; this.toggle.setAttribute('aria-label', t('agent-route.adjust')); this.toggle.setAttribute('aria-haspopup', 'dialog'); this.toggle.setAttribute('aria-expanded', 'false');
    this.modelLabel = text('span', '', 'agent-route-model'); this.effortLabel = text('span', '', 'agent-route-effort'); this.toggle.append(this.modelLabel, this.effortLabel);
    this.menu = text('div', '', 'agent-route-menu'); this.menu.popover = 'auto'; this.menu.dataset.agentPrivate = 'true'; this.menu.setAttribute('role', 'dialog'); this.menu.setAttribute('aria-label', t('agent-route.adjust')); this.menu.hidden = true;
    const field = (label, control) => { const row = text('label', label); control.setAttribute('aria-label', label); row.append(control); this.menu.append(row); return control; };
    this.profile = field(t('agent-route.profile'), document.createElement('select')); this.profile.className = 'agent-profile-select'; this.profile.onchange = () => panel.switchProfile(this.profile.value);
    this.model = document.createElement('input'); this.model.type = 'text'; this.model.autocomplete = 'off'; this.model.spellcheck = false; this.model.setAttribute('list', 'ai-quick-models'); field(t('agent-panel.s98fd0cb'), this.model);
    this.models = document.createElement('datalist'); this.models.id = 'ai-quick-models'; this.menu.append(this.models);
    this.model.onchange = () => panel.setModel(this.model.value);
    this.model.onkeydown = event => { if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); event.stopPropagation(); panel.setModel(this.model.value); this.close(); } };
    this.effort = field(t('agent-route.effort'), document.createElement('select')); this.effort.className = 'agent-effort-select'; this.effort.onchange = () => panel.setReasoning(this.effort.value);
    const configure = text('button', t('agent-dialogues.sd7d7ce7'), 'agent-route-configure'); configure.type = 'button'; configure.onclick = () => { this.close(); panel.configuration(); }; this.menu.append(configure);
    this.toggle.onclick = () => this.opened ? this.close() : this.open();
    this.menu.addEventListener('toggle', event => { this.opened = event.newState === 'open'; this.toggle.setAttribute('aria-expanded', String(this.opened)); });
    this.menu.addEventListener('keydown', event => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); this.close(); this.toggle.focus({ preventScroll: true }); } });
    this.menu.addEventListener('wheel', event => event.stopPropagation(), { passive: true });
    document.addEventListener('pointerdown', event => { if (this.opened && !this.menu.contains(event.target) && !this.toggle.contains(event.target)) this.close(); });
    window.addEventListener('resize', () => { if (this.opened) this.position(); });
    this.update();
  }
  open() {
    if (this.toggle.disabled) return;
    this.update(); this.menu.hidden = false; this.opened = true; this.menu.showPopover?.(); this.position(); this.toggle.setAttribute('aria-expanded', 'true'); this.profile.focus({ preventScroll: true });
  }
  close() { if (!this.opened) return; this.menu.hidePopover?.(); this.opened = false; this.menu.hidden = true; this.toggle.setAttribute('aria-expanded', 'false'); }
  position() {
    const anchor = this.toggle.getBoundingClientRect(), host = this.panel.host.getBoundingClientRect();
    const width = Math.min(320, host.width - 16, window.innerWidth - 16); this.menu.style.width = Math.max(180, width) + 'px';
    const height = this.menu.getBoundingClientRect().height;
    this.menu.style.left = Math.max(8, host.left + 8, Math.min(anchor.right - width, host.right - width - 8, window.innerWidth - width - 8)) + 'px';
    this.menu.style.top = Math.max(8, anchor.top - height - 8) + 'px';
  }
  updateBusy(disabled) {
    this.toggle.disabled = this.profile.disabled = this.model.disabled = disabled;
    this.effort.disabled = disabled || this.panel.reasoningOptions().length < 2;
    if (disabled) this.close();
  }
  update() {
    const panel = this.panel, settings = panel.settings, changedProfile = this.profileId !== settings.activeProfile; this.profileId = settings.activeProfile;
    this.profile.replaceChildren(...settings.profiles.map(profile => { const node = option(profile.id, profile.name); node.disabled = !panel.providers.entries.has(profile.provider); return node; })); this.profile.value = settings.activeProfile;
    if (changedProfile || document.activeElement !== this.model) this.model.value = settings.model;
    this.models.replaceChildren(...[...new Set([settings.model, ...(panel.providers.entries.get(settings.provider)?.models ?? []), ...(panel.capabilities().models ?? [])])].map(value => option(value, agentModelLabel(value))));
    const values = panel.reasoningOptions(), preferred = settings.thinking === false ? 'none' : settings.reasoningEffort, selected = values.includes(preferred) ? preferred : 'auto', defaultEffort = agentEffortDefault(settings, panel.capabilities());
    this.effort.replaceChildren(...values.map(value => option(value, agentEffortLabel(value) + (value === 'auto' && defaultEffort ? ' (' + agentEffortLabel(defaultEffort) + ')' : '')))); this.effort.value = selected;
    this.modelLabel.textContent = agentModelLabel(settings.model);
    this.effortLabel.textContent = selected === 'auto' ? defaultEffort ? agentEffortLabel(defaultEffort) : t('agent-route.defaultShort') : agentEffortLabel(selected);
    this.toggle.title = this.modelLabel.textContent + ' · ' + (this.effort.selectedOptions[0]?.textContent ?? '');
    this.updateBusy(Boolean(panel.agent?.busy || panel.keyLoading || panel.dialogues?.loading)); if (this.opened) this.position();
  }
}
