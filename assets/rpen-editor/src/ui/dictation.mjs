import { DICTATION_DEFAULTS, matchesDictationKey, editableDictationTarget, dictationFieldKind, prepareDictationText } from '../core/dictation.mjs';
import { parseShortcut, formatShortcut } from '../core/shortcut-spec.mjs';
import { recordedShortcut } from '../core/hotkey-settings.mjs';
import { localPreferences } from '../platform/local-preferences.mjs';
import { dictationCall, localDictationAvailable, MicrophoneCapture } from '../platform/dictation.mjs';
import { t } from '../i18n.mjs';
import { cancelValueDrag } from './draggable-value.mjs';

const preferenceKey = 'rpe-next-dictation-v1';
export function readDictationPreferences(storage = localPreferences) {
  try {
    const value = JSON.parse(storage.getItem(preferenceKey) ?? '{}');
    return { enabled: value.enabled !== false, shortcut: !parseShortcut(value.shortcut).error ? value.shortcut : DICTATION_DEFAULTS.shortcut };
  } catch { return { ...DICTATION_DEFAULTS }; }
}

export function createDictationSettings() {
  const section = document.createElement('section'); section.id = 'dictation-settings'; section.className = 'settings-group';
  const heading = document.createElement('div'); heading.className = 'settings-group-heading';
  const title = document.createElement('h3'); title.textContent = t('dictation.title');
  const reset = document.createElement('button'); reset.id = 'dictation-reset'; reset.type = 'button'; reset.className = 'settings-reset'; reset.textContent = t('settings.sa191935'); heading.append(title, reset); section.append(heading);
  for (const [id, key, type] of [['dictation-enabled', 'dictation.enabled', 'checkbox'], ['dictation-shortcut', 'dictation.shortcut', 'text']]) {
    const field = document.createElement('label'); field.className = 'field'; field.append(t(key));
    const input = document.createElement('input'); input.id = id; input.type = type; input.setAttribute('aria-label', t(key)); field.append(input); section.append(field);
  }
  const hint = document.createElement('p'); hint.className = 'hint'; hint.textContent = t('dictation.hint');
  const status = document.createElement('p'); status.id = 'dictation-model-status'; status.className = 'hint dictation-model-status'; status.setAttribute('role', 'status');
  const actions = document.createElement('div'); actions.className = 'storage-actions';
  for (const [id, key] of [['dictation-prepare', 'dictation.prepare'], ['dictation-open', 'dictation.open'], ['dictation-permissions', 'dictation.permissions'], ['dictation-cancel-download', 'dictation.cancelDownload']]) { const button = document.createElement('button'); button.type = 'button'; button.id = id; button.textContent = t(key); actions.append(button); }
  section.append(hint, status, actions); return section;
}

export function snapshotDictationTarget(input) {
  const kind = dictationFieldKind(input);
  if (input.isContentEditable) {
    const selection = input.getRootNode().getSelection?.() ?? window.getSelection();
    const range = selection?.rangeCount ? selection.getRangeAt(0).cloneRange() : null;
    if (!range || !input.contains(range.commonAncestorContainer)) return null;
    return { input, kind, value: input.textContent, range };
  }
  return { input, kind, value: input.value, start: input.selectionStart ?? input.value.length, end: input.selectionEnd ?? input.value.length };
}

export function insertDictation(snapshot, source) {
  const { input, kind } = snapshot;
  if (!input.isConnected || input.disabled || input.readOnly || (snapshot.range ? input.textContent : input.value) !== snapshot.value) return false;
  if (snapshot.range && !input.contains(snapshot.range.commonAncestorContainer)) return false;
  const text = prepareDictationText(source, kind);
  if (text === null) throw Object.assign(Error(t('dictation.invalidNumber')), { candidate: source });
  if (!text) return true;
  const before = new InputEvent('beforeinput', { bubbles: true, cancelable: true, composed: true, inputType: 'insertFromDictation', data: text });
  if (!input.dispatchEvent(before)) return false;
  input.focus({ preventScroll: true });
  const document = input.ownerDocument;
  let inserted = false;
  if (snapshot.range) {
    const selection = input.getRootNode().getSelection?.() ?? window.getSelection(); selection.removeAllRanges(); selection.addRange(snapshot.range);
    inserted = document.execCommand('insertText', false, text);
    if (!inserted) { snapshot.range.deleteContents(); snapshot.range.insertNode(document.createTextNode(text)); }
  } else {
    if (input.selectionStart !== null) input.setSelectionRange(snapshot.start, snapshot.end);
    inserted = document.execCommand('insertText', false, text);
    if (!inserted) {
      if (input.selectionStart === null) input.value = text;
      else input.setRangeText(text, snapshot.start, snapshot.end, 'end');
    }
  }
  if (!inserted) input.dispatchEvent(new InputEvent('input', { bubbles: true, composed: true, inputType: 'insertFromDictation', data: text }));
  return true;
}

export class DictationInput {
  constructor(settings, { notify = () => {}, call = dictationCall, capture = callback => new MicrophoneCapture(callback) } = {}) {
    Object.assign(this, { settings, notify, call, capture });
    this.preferences = readDictationPreferences(); this.session = null; this.recordingShortcut = false;
    this.indicator = document.createElement('div'); this.indicator.className = 'dictation-indicator'; this.indicator.dataset.modInteractive = 'true'; this.indicator.popover = 'manual'; this.indicator.hidden = true; this.indicator.setAttribute('role', 'status'); document.body.append(this.indicator);
    this.candidate = document.createElement('div'); this.candidate.className = 'dictation-candidate'; this.candidate.dataset.modInteractive = 'true'; this.candidate.hidden = true; document.body.append(this.candidate);
    this.bindSettings();
    window.addEventListener('blur', () => this.cancel());
    document.addEventListener('visibilitychange', () => { if (document.hidden) this.cancel(); });
    document.addEventListener('focusin', event => { if (this.session && editableDictationTarget(event) !== this.session.target.input) this.cancel(); }, true);
    globalThis.rpenDictation?.onProgress(value => {
      this.modelState = { ...this.modelState, installation: value, installing: !['ready', 'cancelled', 'error'].includes(value.stage) };
      this.renderModelStatus();
    });
  }
  save() { localPreferences.setItem(preferenceKey, JSON.stringify(this.preferences)); }
  bindSettings() {
    const section = this.settings.querySelector('#dictation-settings');
    const enabled = section.querySelector('#dictation-enabled'); enabled.checked = this.preferences.enabled;
    enabled.onchange = () => { this.preferences.enabled = enabled.checked; this.save(); if (!enabled.checked) this.cancel(); };
    const shortcut = section.querySelector('#dictation-shortcut'); shortcut.value = formatShortcut(this.preferences.shortcut); shortcut.dataset.dictation = 'off'; shortcut.readOnly = true;
    shortcut.onkeydown = event => {
      event.preventDefault(); event.stopImmediatePropagation();
      if (event.key === 'Backspace') this.preferences.shortcut = '';
      else { const result = recordedShortcut(event); if (!result || result.error) return; this.preferences.shortcut = event.code === 'Backquote' && !event.ctrlKey && !event.altKey ? 'TILDE' : result.value; }
      shortcut.value = formatShortcut(this.preferences.shortcut); this.save();
    };
    section.querySelector('#dictation-reset').onclick = () => {
      if (!confirm(t('dictation.resetConfirm'))) return;
      this.preferences = { ...DICTATION_DEFAULTS }; this.save(); enabled.checked = true; shortcut.value = formatShortcut(this.preferences.shortcut);
    };
    const prepare = section.querySelector('#dictation-prepare'); prepare.hidden = !localDictationAvailable();
    prepare.onclick = async () => {
      this.installRequest = true; prepare.disabled = true;
      try { const installed = await this.call('install'); if (installed) await this.call('load'); }
      catch (error) { if (error.name !== 'AbortError') this.notify(error.message, 'error'); }
      finally { this.installRequest = false; await this.refresh(); }
    };
    section.querySelector('#dictation-open').onclick = () => this.call('open').catch(error => this.notify(error.message, 'error'));
    const permissions = section.querySelector('#dictation-permissions'); permissions.hidden = !localDictationAvailable();
    permissions.onclick = () => this.openPermissions().catch(error => this.notify(error.message, 'error'));
    const cancel = section.querySelector('#dictation-cancel-download'); cancel.hidden = true;
    cancel.onclick = async () => {
      cancel.disabled = true;
      if (this.modelState?.installing) { this.modelState.installation = { stage: 'cancelling', cancellable: false }; this.renderModelStatus(); }
      try { await this.call('cancel', 'install'); }
      catch (error) { this.notify(error.message, 'error'); }
      finally { await this.refresh(); }
    };
    this.refresh();
  }
  renderModelStatus() {
    const state = this.modelState ?? {}, value = state.installation;
    const label = this.settings.querySelector('#dictation-model-status');
    const stages = { connecting: 'dictation.connecting', download: 'dictation.downloading', verify: 'dictation.verifying', extract: 'dictation.extracting', install: 'dictation.installing', cancelling: 'dictation.cancelling', cancelled: 'dictation.cancelled' };
    if (value?.stage === 'error') label.textContent = value.error;
    else if (stages[value?.stage]) {
      const details = [];
      label.textContent = t(stages[value.stage]);
      if (value.index) details.push(`${value.index}/${value.count} · ${t(value.kind === 'model' ? 'dictation.modelFile' : 'dictation.runtimeFile')}`);
      if (value.bytes != null) {
        const amount = bytes => (bytes / 1024 ** 2).toFixed(2) + ' MiB';
        details.push(value.total ? `${Math.min(100, value.bytes / value.total * 100).toFixed(1)}% · ${amount(value.bytes)} / ${amount(value.total)}` : amount(value.bytes));
        if (value.stage === 'download') details.push(value.bytesPerSecond >= 1024 ** 2 ? amount(value.bytesPerSecond) + '/s' : Math.round((value.bytesPerSecond || 0) / 1024) + ' KiB/s');
      }
      if (value.source) details.push(value.source);
      if (details.length) label.textContent += '\n' + details.join(' · ');
    } else label.textContent = t(state.ready ? 'dictation.ready' : 'dictation.notReady');
    if (state.root) label.textContent += '\n' + state.root;
    this.settings.querySelector('#dictation-prepare').disabled = Boolean(this.installRequest || state.installing);
    const cancel = this.settings.querySelector('#dictation-cancel-download'); cancel.hidden = !state.installing; cancel.disabled = value?.cancellable === false;
    this.settings.querySelector('#dictation-open').hidden = !state.root;
  }
  async refresh() {
    const label = this.settings.querySelector('#dictation-model-status');
    if (!localDictationAvailable()) { label.textContent = t('dictation.desktopOnly'); this.settings.querySelector('#dictation-open').hidden = true; return; }
    try {
      const state = await this.call('status'); this.ready = state.ready; this.modelState = state; this.renderModelStatus();
    } catch (error) { label.textContent = error.message; }
  }
  handle(event) { event.preventDefault(); event.stopImmediatePropagation(); }
  async openPermissions(status, failure) {
    if (this.permissionDialog?.open) { this.permissionDialog.focus(); return; }
    this.cancel(); cancelValueDrag();
    if (document.pointerLockElement) document.exitPointerLock();
    status ??= await this.call('permissionStatus');
    if (this.permissionDialog?.open) return;
    const dialog = document.createElement('dialog'); dialog.className = 'dictation-permission-dialog'; dialog.dataset.modInteractive = 'true';
    const title = document.createElement('h2'); title.id = 'dictation-permission-title'; title.textContent = t('dictation.permissions'); dialog.setAttribute('aria-labelledby', title.id);
    const explanation = document.createElement('p'); explanation.textContent = t('dictation.permissionHint');
    const state = document.createElement('p'); state.className = 'dictation-permission-state'; state.setAttribute('role', 'status');
    const detail = document.createElement('p'); detail.className = 'hint'; detail.textContent = failure ? `${failure.name}: ${failure.message}` : '';
    const actions = document.createElement('div'); actions.className = 'modal-actions';
    const button = (id, key) => { const node = document.createElement('button'); node.id = id; node.type = 'button'; node.textContent = t(key); actions.append(node); return node; };
    const approve = button('dictation-permission-allow', 'dictation.allowMicrophone');
    const open = button('dictation-permission-open', 'dictation.openMicrophoneSettings');
    const refresh = button('dictation-permission-refresh', 'dictation.recheckPermission');
    const close = button('dictation-permission-close', 'dictation.closePermission');
    const render = value => {
      status = value; approve.hidden = value.consent; open.hidden = !value.canOpenSettings;
      state.textContent = value.blocked ? t(value.platform === 'win32' ? 'dictation.systemDeniedWindows' : value.platform === 'darwin' ? 'dictation.systemDeniedMac' : 'dictation.systemDenied') : !value.consent ? t('dictation.awaitingConsent') : failure ? t('dictation.captureDenied') : t('dictation.permissionReady');
    };
    const run = async callback => {
      for (const node of actions.children) node.disabled = true;
      try { await callback(); } catch (error) { detail.textContent = error.message; }
      finally { for (const node of actions.children) node.disabled = false; }
    };
    approve.onclick = () => run(async () => {
      const result = await this.call('permission', true); render(result);
      if (result.granted) { dialog.close(); this.notify(t('dictation.permissionReady'), 'success'); }
    });
    open.onclick = () => run(async () => { await this.call('openMicrophoneSettings'); });
    refresh.onclick = () => run(async () => { render(await this.call('permissionStatus')); });
    close.onclick = () => dialog.close();
    dialog.addEventListener('close', () => { dialog.remove(); if (this.permissionDialog === dialog) this.permissionDialog = null; }, { once: true });
    dialog.append(title, explanation, state, detail, actions); document.body.append(dialog); this.permissionDialog = dialog;
    render(status); dialog.showModal(); close.focus();
  }
  keydown(event) {
    if (this.session && event.key === 'Escape') { this.handle(event); this.cancel(); return; }
    if (!this.preferences.enabled || event.isComposing || this.recordingShortcut || event.target?.id === 'dictation-shortcut' || document.querySelector('.hotkey-dialog[open]')) return;
    if (!matchesDictationKey(event, this.preferences.shortcut)) return;
    const input = editableDictationTarget(event); if (!input || event.defaultPrevented) return;
    this.handle(event); if (event.repeat || this.session) return;
    const target = snapshotDictationTarget(input); if (!target) return;
    this.begin(target, event.code).catch(error => { if (error.name !== 'AbortError') this.notify(error.message, 'error'); });
  }
  keyup(event) {
    if (!this.session || event.code !== this.session.code) return;
    this.handle(event); this.session.released = true;
    if (this.session.phase === 'recording') this.finish();
  }
  show(text, phase = 'preparing') { this.indicator.dataset.phase = phase; this.indicator.textContent = text; this.indicator.hidden = false; this.indicator.showPopover?.(); }
  hide() { this.indicator.hidePopover?.(); this.indicator.hidden = true; }
  async begin(target, code) {
    const state = { target, code, phase: 'preparing', released: false, cancelled: false, queue: Promise.resolve(), token: null }; this.session = state;
    try {
      if (!localDictationAvailable()) throw Error(t('dictation.desktopOnly'));
      const status = await this.call('status');
      if (state.cancelled || state.released) return;
      if (!status.ready) {
        this.notify(t('dictation.prepareFirst'), 'warning');
        if (!this.settings.open) this.settings.showModal();
        requestAnimationFrame(() => this.settings.querySelector('#dictation-settings').scrollIntoView({ block: 'center' })); return;
      }
      this.show(t('dictation.preparing'));
      const access = await this.call('permissionStatus');
      if (state.cancelled || state.released) return;
      if (!access.consent || access.blocked) { await this.openPermissions(access); return; }
      const permission = await this.call('permission');
      if (!permission.granted) { await this.openPermissions(permission); return; }
      if (state.cancelled || state.released) return;
      state.engine = this.call('begin').then(token => { state.token = token; if (state.cancelled) this.call('cancel', token).catch(() => {}); });
      state.engine.catch(error => { if (!state.cancelled) { this.notify(error.message, 'error'); this.cancel(); } });
      state.microphone = this.capture((samples, sampleRate) => {
        state.queue = state.queue.then(async () => { await state.engine; if (!state.cancelled && state.token) await this.call('write', state.token, samples, sampleRate); }).catch(error => { if (!state.cancelled) { this.notify(error.message, 'error'); this.cancel(); } });
      });
      await state.microphone.start();
      if (state.cancelled || state.released) return;
      state.phase = 'recording'; this.show(t('dictation.listening'), 'recording');
      state.timer = setTimeout(() => this.finish(), DICTATION_DEFAULTS.maximumSeconds * 1000);
      return await new Promise(resolve => { state.completed = resolve; });
    } catch (error) {
      if (!state.cancelled && ['NotAllowedError', 'SecurityError', 'PermissionDeniedError'].includes(error.name)) { await this.openPermissions(undefined, error); return; }
      if (!state.cancelled) throw error;
    } finally {
      if (state.phase !== 'recording' && state.phase !== 'finishing') {
        state.cancelled = true;
        await state.microphone?.stop(); if (state.token) await this.call('cancel', state.token).catch(() => {});
        if (this.session === state) { this.session = null; this.hide(); }
      }
    }
  }
  async finish() {
    const state = this.session; if (!state || state.phase !== 'recording') return;
    state.phase = 'finishing'; clearTimeout(state.timer); this.show(t('dictation.recognizing'), 'finishing');
    try {
      await state.microphone.stop(); await state.engine; await state.queue;
      if (state.cancelled) return;
      const text = await this.call('finish', state.token);
      if (state.cancelled || this.session !== state) return;
      if (!text?.trim()) { this.notify(t('dictation.empty'), 'warning'); return; }
      try { if (!insertDictation(state.target, text)) this.showCandidate(text); }
      catch (error) { this.notify(error.message, 'warning'); this.showCandidate(text); }
    } catch (error) { if (!state.cancelled) this.notify(error.message, 'error'); }
    finally { if (this.session === state) { this.session = null; this.hide(); } state.phase = 'done'; state.completed?.(); }
  }
  showCandidate(text) {
    const value = document.createElement('textarea'); value.value = text; value.readOnly = true;
    const close = document.createElement('button'); close.textContent = '×'; close.onclick = () => { this.candidate.hidden = true; };
    const label = document.createElement('span'); label.textContent = t('dictation.candidate'); this.candidate.replaceChildren(label, value, close); this.candidate.hidden = false;
  }
  cancel() {
    const state = this.session; if (!state) return;
    state.cancelled = true; state.phase = 'cancelled'; clearTimeout(state.timer); state.microphone?.stop();
    if (state.token) this.call('cancel', state.token).catch(() => {});
    this.session = null; this.hide(); state.completed?.();
  }
}
