// Offline workerd integration: real DO storage/alarms, fake upstreams and keys.
import assert from 'node:assert/strict';
import { createServer, request } from 'node:http';
import { createHash, createHmac, randomBytes } from 'node:crypto';
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { spawn } from 'node:child_process';
import { setTimeout as delay } from 'node:timers/promises';

const root = resolve(import.meta.dirname, '..');
const fixtures = join(root, 'tests/fixtures');
const auth = JSON.parse(await readFile(join(fixtures, 'webhook/auth.json')));
const claims = Buffer.from(JSON.stringify({did:auth.did, exp:Math.floor(Date.now()/1000) + 300})).toString('base64url');
const session = claims + '.' + createHmac('sha256', auth.secret).update(claims).digest('base64url');
const target = await readFile(join(fixtures, 'vault/target.md'), 'utf8');
const retitled = await readFile(join(fixtures, 'vault/retitled.md'), 'utf8');
const files = new Map([['target', target]]);
let embeddings = 0, unavailable = false, unavailableReads = 0;
const blob = text => createHash('sha1').update(`blob ${Buffer.byteLength(text)}\0`).update(text).digest('hex');
const upstream = createServer(async (req, res) => {
  try {
    let body = ''; for await (const part of req) body += part;
    const url = new URL(req.url, 'http://test');
    let result;
    if (url.pathname.includes('/git/trees/')) {
      if (unavailable) { unavailableReads++; res.writeHead(404).end(); return; }
      result = { truncated:false, tree:[...files].map(([id, text]) => ({ path:`notes/${id}.md`, type:'blob', sha:blob(text) })) };
    } else if (url.pathname.includes('/contents/notes/')) {
      const id = decodeURIComponent(url.pathname.split('/contents/notes/')[1]).replace(/\.md$/, '');
      if (unavailable) { res.writeHead(404).end(); return; }
      if (req.method === 'PUT') {
        files.set(id, Buffer.from(JSON.parse(body).content, 'base64').toString());
        result = { commit:{sha:'test-commit'} };
      } else {
        if (!files.has(id)) { res.writeHead(404).end(); return; }
        const text = files.get(id);
        result = { sha:blob(text), encoding:'base64', content:Buffer.from(text).toString('base64') };
      }
    } else if (url.pathname.endsWith('/embeddings')) {
      embeddings++;
      const input = JSON.parse(body).input;
      result = { data:input.map((_, index) => ({ index, embedding:[1, 0] })) };
    } else { throw new Error(`unexpected upstream ${url.pathname}`); }
    res.writeHead(200, { 'Content-Type':'application/json' }).end(JSON.stringify(result));
  } catch (error) { res.writeHead(500).end(error.message); }
});
await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
const mock = `http://127.0.0.1:${upstream.address().port}`;
const temp = await mkdtemp(join(tmpdir(), 'notes-runtime-'));
let child, socket, log = '';
try {
  const shim = (await readFile(join(fixtures, 'runtime/shim.mjs'), 'utf8'))
    .replaceAll('__WORKER__', join(root, 'build/worker/shim.mjs')).replaceAll('__UPSTREAM__', mock);
  await writeFile(join(temp, 'shim.mjs'), shim);
  await writeFile(join(temp, 'wrangler.toml'), (await readFile(join(fixtures, 'runtime/wrangler.toml'), 'utf8')).replace('__SHIM__', join(temp, 'shim.mjs')));
  const env = { ...process.env, HOME:temp, XDG_CONFIG_HOME:temp, CI:'1', WRANGLER_SEND_METRICS:'false' };
  for (const key of Object.keys(env)) if (/(TOKEN|SECRET|API_KEY|PASSWORD)/i.test(key)) delete env[key];
  const ready = new Promise((resolve, reject) => {
    child = spawn('wrangler', ['dev', '--local', '--ip', '127.0.0.1', '--port', '0', '--config', join(temp, 'wrangler.toml')], { cwd:temp, env, detached:true, stdio:['ignore','pipe','pipe'] });
    const capture = chunk => {
      log = (log + chunk).slice(-20000);
      const match = log.match(/Ready on (http:\/\/127\.0\.0\.1:\d+)/);
      if (match) resolve(match[1]);
    };
    child.stdout.on('data', capture); child.stderr.on('data', capture);
    child.once('error', reject); child.once('exit', code => reject(new Error(`wrangler exited ${code}`)));
  });
  const base = await Promise.race([ready, delay(60000, undefined, {ref:false}).then(() => { throw new Error('runtime startup timeout'); })]);
  const api = async (path, body, status = 200) => {
    const response = await fetch(base + path, { method:body === undefined ? 'GET' : 'POST', headers:{ Cookie:`session=${session}`, 'Content-Type':'application/json' }, body:body === undefined ? undefined : JSON.stringify(body) });
    assert.equal(response.status, status, `${path}: ${await response.clone().text()}`);
    return response.json();
  };
  const idle = async () => {
    for (let n = 0; n < 300; n++) {
      if ((await api('/vault/status')).pending === 0) return;
      await delay(100);
    }
    throw new Error('queue did not drain');
  };
  const push = async (forced = false) => {
    const body = JSON.stringify({ repository:{full_name:'example/notes'}, ref:'refs/heads/main', forced, commits:[{modified:['notes/target.md']}] });
    const signature = createHmac('sha256', auth.webhook_key).update(body).digest('hex');
    const response = await fetch(base + '/vault/events/github', { method:'POST', headers:{ 'X-Hub-Signature-256':`sha256=${signature}`, 'X-GitHub-Event':'push' }, body });
    assert.equal(response.status, 200, await response.text());
    await idle();
  };
  assert.equal((await fetch(base + '/vault/verify', {method:'POST',body:'{}'})).status, 401);
  const cors = await fetch(base + '/vault/search', {method:'OPTIONS',headers:{Origin:'https://notes.example'}});
  assert.equal(cors.status, 200); assert.match(cors.headers.get('Access-Control-Allow-Methods'), /POST/);
  assert.equal((await api('/vault/verify', {})).rows[0].reasons[0], 'missing');
  await api('/vault/verify', {repair:true}); await idle();
  assert.equal((await api('/vault/verify', {})).clean, true);
  assert.equal(embeddings, 1);
  // Open hydrates the cold Git body through an append, not a seed/reset.
  const connect = () => new Promise((resolve, reject) => {
    const req = request(base + '/note/target/ws', {headers:{ Cookie:`session=${session}`, Upgrade:'websocket', Connection:'Upgrade', 'Sec-WebSocket-Key':randomBytes(16).toString('base64'), 'Sec-WebSocket-Version':'13' }});
    req.on('upgrade', (_, socket) => { socket.setTimeout(0); resolve(socket); }); req.on('error', reject);
    req.on('response', response => reject(new Error(`upgrade failed: ${response.statusCode}`)));
    req.setTimeout(5000, () => req.destroy(new Error('socket upgrade timeout'))); req.end();
  });
  const send = (socket, message) => {
    const payload = Buffer.from(JSON.stringify(message)), mask = randomBytes(4);
    const header = payload.length < 126 ? Buffer.from([0x81, 0x80 | payload.length]) : Buffer.from([0x81, 0xfe, payload.length >> 8, payload.length & 255]);
    socket.write(Buffer.concat([header, mask, Buffer.from(payload.map((value, index) => value ^ mask[index % 4]))]));
  };
  socket = await connect();
  send(socket, {type:'open', since_seq:0});
  for (let n = 0; n < 100; n++) { if ((await api('/__test/snapshot')).seq === 1) break; await delay(100); }
  assert.equal((await api('/__test/snapshot')).text, target);
  await idle();
  await push(true); // Durable whole-vault discovery, including status metadata.
  assert.equal(embeddings, 1);
  files.set('target', retitled); await push();
  const conflict = (await api('/vault/verify', {})).rows[0];
  assert(conflict.reasons.includes('git_conflict')); assert.equal(conflict.repairable, false);
  assert.equal((await api('/__test/snapshot')).text, target);
  socket.destroy(); socket = undefined;
  for (let n = 0; n < 100; n++) { if ((await api('/__test/snapshot')).active_sockets === 0) break; await delay(100); }
  await api('/vault/verify', {repair:true}); await idle();
  assert.equal((await api('/vault/verify', {})).clean, true);
  assert.equal((await api('/__test/snapshot')).seq, 2);
  assert.equal(embeddings, 1, 'metadata-only import must reuse its vector');
  assert.equal((await api('/vault/search', {query:'stable prose'})).hits[0].id, 'target');
  assert.equal((await api('/vault/search', {query:'similar ideas',mode:'semantic'})).hits[0].id, 'target');
  // A normal dirty hot note leads Git legitimately, without a false conflict.
  socket = await connect();
  send(socket, {type:'edit', base_seq:2, delta:{kind:'whole', text:retitled + '\nPending hot prose.\n'}});
  for (let n = 0; n < 100; n++) { if ((await api('/__test/snapshot')).seq === 3) break; await delay(100); }
  send(socket, {type:'open', since_seq:0});
  for (let n = 0; n < 100; n++) { if ((await api('/vault/verify', {})).clean) break; await delay(100); }
  assert.equal((await api('/vault/verify', {})).clean, true);
  socket.destroy(); socket = undefined;
  for (let n = 0; n < 100; n++) { if ((await api('/__test/snapshot')).committed_seq === 3) break; await delay(100); }
  assert.equal((await api('/__test/snapshot')).committed_seq, 3);
  // Authentication/repository failures are not interpreted as deletions.
  unavailable = true;
  const event = JSON.stringify({repository:{full_name:'example/notes'},ref:'refs/heads/main',commits:[{modified:['notes/target.md']}]});
  const signature = createHmac('sha256', auth.webhook_key).update(event).digest('hex');
  await fetch(base + '/vault/events/github', {method:'POST',headers:{'X-Hub-Signature-256':`sha256=${signature}`,'X-GitHub-Event':'push'},body:event});
  for (let n = 0; n < 100 && !unavailableReads; n++) await delay(100);
  assert(unavailableReads > 0);
  assert.equal((await api('/__test/snapshot')).deleted, false);
  assert.equal((await api('/__test/snapshot')).seq, 3);
  unavailable = false; await push();
  files.delete('target'); await push();
  const deleted = await api('/__test/snapshot');
  assert.equal(deleted.seq, 4); assert.equal(deleted.deleted, true);
  assert.equal((await api('/vault/search', {query:'stable'})).hits.length, 0);
  assert.equal((await api('/vault/verify', {})).clean, true);
  console.log('runtime smoke passed: repair, open hydration, conflicts, metadata reuse, text/semantic search, deletion');
} catch (error) { console.error(log); throw error; }
finally {
  socket?.destroy();
  if (child && child.exitCode === null && child.signalCode === null) { process.kill(-child.pid, 'SIGTERM'); await new Promise(resolve => child.once('exit', resolve)); }
  upstream.closeAllConnections(); await new Promise(resolve => upstream.close(resolve));
  await rm(temp, { recursive:true, force:true });
}
