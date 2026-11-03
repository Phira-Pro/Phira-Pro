import { t } from '../i18n.mjs';
export async function audioStorageCall(action, options, bridge = globalThis.rpenAudioStorage) {
  if (bridge?.version !== 1) throw Error(t('audio-storage.s5425829'));
  const response = await bridge.call(action, options);
  if (!response.ok) throw Object.assign(Error(response.error.message), { name: response.error.name });
  return response.result;
}

export async function prepareAudioStorage(bridge = globalThis.rpenAudioStorage) {
  return bridge?.version === 1 ? audioStorageCall('prepare', undefined, bridge) : true;
}
