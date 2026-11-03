import { APP_VERSION, UPDATE_INTERVAL, UPDATE_MANUAL_INTERVAL, UPDATE_RELEASES_URL, selectUpdateRelease, publicUpdateInfo } from '../core/app-update.mjs';

export class UpdateChecker {
  constructor({ version = APP_VERSION, platform = 'web', arch = '', fetcher = globalThis.fetch, state = {}, persist = async () => {}, now = Date.now } = {}) {
    if (!state || typeof state !== 'object') state = {};
    Object.assign(this, { version, platform, arch, fetcher, persist, now });
    this.state = { autoCheck: state.autoCheck !== false, checked: Number(state.checked) || 0, attempted: Number(state.attempted) || 0, retryAt: Number(state.retryAt) || 0, etag: typeof state.etag === 'string' ? state.etag.slice(0, 256) : '' };
    // Re-parse cached metadata with exactly the same URL/platform policy as a
    // network response. A settings file never supplies an arbitrary download.
    try { this.state.latest = selectUpdateRelease(state.releases ?? [], platform, arch); this.state.releases = state.releases; } catch { this.state.latest = null; }
  }
  info() { return publicUpdateInfo(this.state, this.version); }
  async setAuto(value) { this.state.autoCheck = Boolean(value); await this.persist(this.state); return this.info(); }
  async check(manual = false) {
    if (this.pending) return this.pending;
    const now = this.now(), gap = manual ? UPDATE_MANUAL_INTERVAL : UPDATE_INTERVAL;
    if ((!manual && !this.state.autoCheck) || now < this.state.retryAt || (this.state.attempted && now - this.state.attempted < gap)) return this.info();
    this.pending = this.request(now).finally(() => { this.pending = null; }); return this.pending;
  }
  async request(now) {
    this.state.attempted = now;
    try {
      const headers = { Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' };
      if (this.state.etag) headers['If-None-Match'] = this.state.etag;
      const response = await this.fetcher(UPDATE_RELEASES_URL, { headers, credentials: 'omit', cache: 'no-store', signal: AbortSignal.timeout(20000) });
      if (response.status === 403 || response.status === 429) {
        const seconds = Number(response.headers.get('retry-after')), reset = Number(response.headers.get('x-ratelimit-reset')) * 1000;
        this.state.retryAt = Math.min(now + UPDATE_INTERVAL, Math.max(now + UPDATE_MANUAL_INTERVAL, seconds > 0 ? now + seconds * 1000 : reset || now + 3600000));
        throw Object.assign(Error('Update check rate limited'), { code: 'UPDATE_RATE_LIMIT' });
      }
      if (response.status !== 304) {
        if (!response.ok) throw Error('Update feed HTTP ' + response.status);
        const source = await limitedResponseText(response, 4 * 1024 * 1024), releases = JSON.parse(source);
        const latest = selectUpdateRelease(releases, this.platform, this.arch);
        // Store only fields used by the updater, not the full public API body.
        this.state.releases = latest ? releases.filter(r => r.tag_name === latest.tag).slice(0, 1).map(r => ({ tag_name: r.tag_name, body: latest.notes, draft: false, assets: (r.assets ?? []).filter(a => a.name === latest.asset?.name || a.name === 'SHA256SUMS.txt').map(({ name, size, digest, browser_download_url }) => ({ name, size, digest, browser_download_url })) })) : [];
        this.state.latest = latest; this.state.etag = response.headers.get('etag') ?? '';
      }
      this.state.checked = now; this.state.retryAt = 0; return this.info();
    } catch (error) {
      this.state.retryAt = Math.max(this.state.retryAt, now + UPDATE_MANUAL_INTERVAL);
      throw Object.assign(Error('Cannot check for updates'), { code: error.code === 'UPDATE_RATE_LIMIT' ? error.code : 'UPDATE_NETWORK' });
    } finally { await this.persist(this.state); }
  }
}

export async function limitedResponseText(response, limit) {
  let size = 0; const reader = response.body.getReader(), chunks = [], decoder = new TextDecoder();
  try { for (;;) { const { done, value } = await reader.read(); if (done) break; size += value.byteLength; if (size > limit) throw Error('Update response too large'); chunks.push(decoder.decode(value, { stream: true })); } return chunks.join('') + decoder.decode(); }
  finally { await reader.cancel().catch(() => {}); }
}
