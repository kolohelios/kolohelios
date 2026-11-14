// Test-only transport interception. Unknown outbound URLs fail closed.
import service, { VaultDurableObject as Vault } from '__WORKER__';
export * from '__WORKER__';
const originalFetch = globalThis.fetch.bind(globalThis);
globalThis.fetch = (input, init) => {
  const request = new Request(input, init);
  const url = new URL(request.url);
  if (!['api.github.com', 'openrouter.ai'].includes(url.hostname)) {
    throw new Error('unexpected outbound request in offline runtime test');
  }
  return originalFetch(new Request('__UPSTREAM__' + url.pathname + url.search, request));
};
export class VaultDurableObject extends Vault {
  async fetch(request) {
    try { return await super.fetch(request); }
    catch (error) { console.error('test Vault error', error); throw error; }
  }
}
export default class extends service {
  fetch(request) {
    if (new URL(request.url).pathname === '/__test/snapshot') {
      return this.env.NOTE.get(this.env.NOTE.idFromName('target')).fetch('https://note/__export');
    }
    return super.fetch(request);
  }
}
