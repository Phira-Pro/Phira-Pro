// Register the host escape before editor and extension input capture listeners.
// A modal extension may block every editor control, including its own prompt.
import { localizedValue } from '../src/i18n.mjs';
export async function disableModalOwner(manager, owner) {
  if (!owner || !manager.runtime.loaded.has(owner)) return;
  await manager.disable(owner);
  manager.renderList();
}

export function installRecovery() {
  let trigger, menu, owner, scheduled = false;
  const api = () => globalThis.rpenMods;
  const hide = () => { if (trigger?.matches(':popover-open')) trigger.hidePopover(); };
  const sync = () => {
    scheduled = false;
    if (!trigger || menu.open) { hide(); return; }
    const modals = [...document.querySelectorAll('dialog[open]')].filter(node => node.matches(':modal') && api()?.runtime.ownerOf(node));
    const active = document.activeElement?.closest?.('dialog:modal');
    const modal = modals.includes(active) ? active : modals.at(-1);
    if (!modal) { hide(); if (trigger.parentElement !== document.body) document.body.append(trigger); owner = null; return; }
    owner = api().runtime.ownerOf(modal);
    if (trigger.parentElement !== modal) { hide(); modal.append(trigger); }
    if (!trigger.matches(':popover-open')) trigger.showPopover();
  };
  const schedule = () => { if (!scheduled) { scheduled = true; requestAnimationFrame(sync); } };
  const close = () => { menu.close(); schedule(); };
  const run = async action => {
    if (action === 'open') {
      menu.querySelector('[data-owner]').textContent = localizedValue(api()?.manager.records.get(owner)?.manifest.name) || owner;
      hide(); menu.showModal(); return;
    }
    if (action === 'cancel') { close(); return; }
    try {
      if (action === 'disable') {
        await disableModalOwner(api().manager, owner);
        close(); api().editor.notify('已停用遮挡界面的扩展，可继续编辑工程', 'success');
      } else if (action === 'return') await globalThis.phiraEditor.requestClose();
    } catch (error) { menu.querySelector('[role=status]').textContent = error.message; }
  };
  for (const type of ['pointerdown','pointerup','mousedown','mouseup','click','touchstart','touchend','wheel','keydown','keyup']) {
    window.addEventListener(type, event => {
      const path = event.composedPath();
      if (!path.includes(trigger) && !path.includes(menu)) return;
      event.stopImmediatePropagation();
      if (type === 'click') {
        const action = path.find(node => node.dataset?.proRecoveryAction)?.dataset.proRecoveryAction;
        if (action) { event.preventDefault(); run(action); }
      }
      if (type === 'keydown' && event.key === 'Escape' && menu.open) { event.preventDefault(); close(); }
    }, { capture: true, passive: false });
  }
  const build = () => {
    trigger = document.createElement('div'); trigger.id = 'pro-extension-recovery'; trigger.popover = 'manual';
    trigger.style.cssText = 'position:fixed;inset:8px 8px auto auto;margin:0;padding:0;border:0;background:transparent;max-width:calc(100vw - 16px);';
    const shadow = trigger.attachShadow({ mode: 'open' });
    const sheet = document.createElement('style');
    sheet.textContent = 'button{all:initial;display:block;font:13px/1.4 system-ui;color:#fff;background:#303945;border:1px solid #8d9caf;border-radius:6px;padding:5px 9px;cursor:pointer}button:focus-visible{outline:2px solid #f3d483}';
    const button = document.createElement('button'); button.type = 'button'; button.textContent = '扩展控制'; button.dataset.proRecoveryAction = 'open';
    shadow.append(sheet, button);
    menu = document.createElement('dialog'); menu.id = 'pro-extension-recovery-menu'; menu.className = 'pro-dialog'; menu.dataset.modInteractive = 'true';
    menu.innerHTML = '<h2>扩展控制</h2><p data-owner></p><p>可停用遮挡界面的扩展，保留当前工程和扩展安装包。</p><p role="status"></p><div class="pro-actions"><button type="button" data-pro-recovery-action="cancel">取消</button><button type="button" data-pro-recovery-action="return">返回 Pro</button><button type="button" class="primary" data-pro-recovery-action="disable">停用此扩展</button></div>';
    menu.addEventListener('cancel', event => { event.preventDefault(); close(); });
    document.body.append(trigger, menu);
    new MutationObserver(records => {
      if (records.some(record => record.type === 'attributes' || [...record.addedNodes, ...record.removedNodes].some(node => node.nodeType === 1 && (node.matches('dialog') || node.querySelector('dialog'))))) schedule();
    }).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ['open'] });
    schedule();
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', build, { once: true }); else build();
}

if (typeof window !== 'undefined' && typeof document !== 'undefined') installRecovery();
