const messages = {
  openMarket: ['模组市场', 'Mod market'],
  releasePage: ['模组发布页', 'Mod release page'],
  checkUpdates: ['检查更新', 'Check for updates'],
  updatesAvailable: ['更新 · {count}', 'Updates · {count}'],
  stopped: ['市场操作已取消', 'Market operation cancelled'],
  recoveryPending: ['旧包恢复未完成', 'Previous package recovery is pending'],
  recoverySaved: ['旧包恢复未完成，备份已保留', 'Previous package recovery is pending. Backups are retained.'],
  recoveryRestored: ['旧包已恢复，保持禁用', 'Previous packages restored and left disabled.'],
  recoveryUnavailable: ['恢复备份存储不可用', 'Recovery backup storage is unavailable'],
  restorePackages: ['恢复旧包', 'Restore previous packages'],
  exportPackages: ['导出旧包', 'Export previous packages'],
  retry: ['重试', 'Retry'],
  failed: ['操作未完成', 'Operation failed'],
  rolledBack: ['安装未完成，已恢复原版本', 'Installation failed. Previous versions have been restored.'],
  installFailed: ['安装未完成', 'Installation failed'],
  title: ['OpenRPEN · 模组市场', 'OpenRPEN · Mod market'],
  search: ['搜索模组', 'Search mods'],
  allCategories: ['全部类别', 'All categories'],
  allMods: ['全部模组', 'All mods'],
  updates: ['更新', 'Updates'],
  updateDisclosure: ['检查时向 {endpoint} 发送所选模组的 ID、版本和内容指纹', 'Checking sends the selected mods’ IDs, versions and content hashes to {endpoint}.'],
  marketOnly: ['仅本市场来源', 'From this market only'],
  manualSelection: ['手动选择', 'Choose mods'],
  back: ['← 模组市场', '← Mod market'],
  reinstall: ['重新安装', 'Reinstall'],
  install: ['安装', 'Install'],
  feedback: ['反馈', 'Feedback'],
  copyRelease: ['复制发布页', 'Copy release URL'],
  history: ['版本记录', 'Version history'],
  installVersion: ['安装此版本', 'Install this version'],
  feedbackPlaceholder: ['复现步骤、预期表现、实际表现', 'Steps to reproduce, expected behavior and actual behavior'],
  send: ['发送', 'Send'],
  restoreEnabled: ['安装后恢复启用：{mods}（会执行新版本）', 'Re-enable after installation: {mods} (executes the new versions)'],
  replacing: ['替换本机 {name} {version}', 'Replaces installed {name} {version}'],
  managerBusy: ['模组管理器正在处理其他操作', 'The mod manager is busy'],
};

let language = 0;
let localize = value => {
  if (typeof value === 'string') return value;
  if (!value || typeof value !== 'object') return '';
  return value[language ? 'en-US' : 'zh-CN'] ?? value['zh-CN'] ?? Object.values(value).find(text => typeof text === 'string') ?? '';
};

export function configureMarketText(locale, resolve) {
  language = /^en(?:-|$)/i.test(locale) ? 1 : 0;
  if (resolve) localize = resolve;
}

export function marketText(key, values = {}) {
  return (messages[key]?.[language] ?? key).replace(/\{([a-zA-Z]+)\}/g, (placeholder, name) => values[name] === undefined ? placeholder : String(values[name]));
}

export const marketName = value => localize(value);

export async function runMarketTask(task) {
  try {
    return await task();
  } catch (error) {
    if (error?.name !== 'AbortError') throw error;
  }
}
