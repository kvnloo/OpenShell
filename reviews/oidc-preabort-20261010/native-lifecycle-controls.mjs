import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { setTimeout as delay } from 'node:timers/promises';
import { clientCredentials } from '@nvidia/openshell-sdk';

// Independent public-package self-reference. Run from inside each isolated SDK
// package with Node --unhandled-rejections=strict. No fetch or rejection mocks.
const deadline = setTimeout(() => {
  throw new Error('native lifecycle controls exceeded 12 second process deadline');
}, 12_000);
deadline.unref();

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function fixture(tokenHandler, discoveryHandler) {
  const counts = { discovery: 0, token: 0, secret: 0 };
  let issuer;
  const server = createServer((req, res) => {
    if (req.url === '/.well-known/openid-configuration') {
      counts.discovery += 1;
      if (discoveryHandler?.(res, counts)) return;
      res.end(JSON.stringify({ issuer, token_endpoint: `${issuer}/token` }));
      return;
    }
    assert.equal(req.url, '/token');
    assert.equal(req.method, 'POST');
    let body = '';
    req.setEncoding('utf8');
    req.on('data', (chunk) => { body += chunk; });
    req.on('end', () => {
      const form = new URLSearchParams(body);
      assert.equal(form.get('grant_type'), 'client_credentials');
      assert.equal(form.get('client_secret'), 'synthetic-secret');
      counts.token += 1;
      if (tokenHandler?.(res, counts)) return;
      res.end(JSON.stringify({ access_token: `native-token-${counts.token}`, expires_in: 120 }));
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  issuer = `http://127.0.0.1:${server.address().port}`;
  return {
    counts,
    provider(overrides = {}) {
      return clientCredentials({
        issuer, clientId: 'synthetic-client', timeoutMs: 500,
        clientSecret: () => { counts.secret += 1; return 'synthetic-secret'; },
        ...overrides,
      });
    },
    async close() {
      server.closeAllConnections();
      await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
      assert.equal(server.listening, false);
    },
  };
}

async function run(name, action) {
  const result = await action();
  console.log(JSON.stringify({ name, status: 'PASS', ...result }));
}

try {
  await run('normal-coalescing-and-cached-preabort', async () => {
    const f = await fixture();
    try {
      const p = f.provider();
      assert.deepEqual(await Promise.all(Array.from({ length: 8 }, () => p.getToken())), Array(8).fill('native-token-1'));
      assert.equal(await p.getToken(AbortSignal.abort()), 'native-token-1');
      assert.deepEqual(f.counts, { discovery: 1, token: 1, secret: 1 });
      return { counts: f.counts };
    } finally { await f.close(); }
  });

  await run('preaborted-join-and-mixed-active-canceled-live', async () => {
    const started = deferred();
    let release;
    const f = await fixture((res) => {
      release = () => { if (!res.writableEnded) res.end(JSON.stringify({ access_token: 'native-shared', expires_in: 120 })); };
      started.resolve();
      return true;
    });
    try {
      const p = f.provider();
      const firstLive = p.getToken();
      await started.promise;
      await assert.rejects(p.getToken(AbortSignal.abort()), { code: 'canceled' });
      const controller = new AbortController();
      const canceled = assert.rejects(p.getToken(controller.signal), { code: 'canceled' });
      const secondLive = p.getToken();
      controller.abort();
      await canceled;
      release();
      assert.deepEqual(await Promise.all([firstLive, secondLive]), ['native-shared', 'native-shared']);
      assert.equal(await p.getToken(AbortSignal.abort()), 'native-shared');
      assert.deepEqual(f.counts, { discovery: 1, token: 1, secret: 1 });
      return { counts: f.counts };
    } finally { release?.(); await f.close(); }
  });

  await run('all-started-waiters-cancel-token503-clean-retry', async () => {
    const started = deferred();
    let release;
    const f = await fixture((res, counts) => {
      if (counts.token !== 1) return false;
      release = () => { if (!res.writableEnded) { res.writeHead(503); res.end('synthetic unavailable token'); } };
      started.resolve();
      return true;
    });
    try {
      const p = f.provider();
      const c1 = new AbortController();
      const c2 = new AbortController();
      const canceled1 = assert.rejects(p.getToken(c1.signal), { code: 'canceled' });
      const canceled2 = assert.rejects(p.getToken(c2.signal), { code: 'canceled' });
      await started.promise;
      c1.abort(); c2.abort();
      await Promise.all([canceled1, canceled2]);
      release();
      // A real loopback response must finish after every public waiter canceled.
      // Strict Node rejection mode remains enabled through failure and retry.
      await delay(200);
      assert.equal(await p.getToken(), 'native-token-2');
      assert.deepEqual(f.counts, { discovery: 1, token: 2, secret: 2 });
      return { counts: f.counts };
    } finally { release?.(); await f.close(); }
  });

  await run('single-started-waiter-cancels-success-remains-cacheable', async () => {
    const started = deferred();
    let release;
    const f = await fixture((res) => {
      release = () => { if (!res.writableEnded) res.end(JSON.stringify({ access_token: 'native-background', expires_in: 120 })); };
      started.resolve();
      return true;
    });
    try {
      const p = f.provider();
      const c = new AbortController();
      const canceled = assert.rejects(p.getToken(c.signal), { code: 'canceled' });
      await started.promise;
      c.abort();
      await canceled;
      release();
      await delay(200);
      assert.equal(await p.getToken(), 'native-background');
      assert.deepEqual(f.counts, { discovery: 1, token: 1, secret: 1 });
      return { counts: f.counts };
    } finally { release?.(); await f.close(); }
  });

  await run('active-cancel-stalled-discovery-body-timeout-clean-retry', async () => {
    const started = deferred();
    const f = await fixture(undefined, (res, counts) => {
      if (counts.discovery !== 1) return false;
      res.writeHead(200, { 'content-type': 'application/json' });
      res.write('{"issuer":');
      started.resolve();
      return true;
    });
    try {
      const p = f.provider({ timeoutMs: 80 });
      const c = new AbortController();
      const canceled = assert.rejects(p.getToken(c.signal), { code: 'canceled' });
      await started.promise;
      c.abort();
      await canceled;
      await delay(250);
      assert.equal(await p.getToken(), 'native-token-1');
      assert.deepEqual(f.counts, { discovery: 2, token: 1, secret: 1 });
      return { counts: f.counts };
    } finally { await f.close(); }
  });

  await run('active-cancel-async-secret-rejection-clean-retry', async () => {
    const secretStarted = deferred();
    const secretValue = deferred();
    const f = await fixture();
    try {
      const p = f.provider({ clientSecret: () => {
        f.counts.secret += 1;
        if (f.counts.secret === 1) { secretStarted.resolve(); return secretValue.promise; }
        return 'synthetic-secret';
      } });
      const c = new AbortController();
      const canceled = assert.rejects(p.getToken(c.signal), { code: 'canceled' });
      await secretStarted.promise;
      c.abort();
      await canceled;
      secretValue.reject(new Error('synthetic supplier failure'));
      await delay(100);
      assert.equal(await p.getToken(), 'native-token-1');
      assert.deepEqual(f.counts, { discovery: 1, token: 1, secret: 2 });
      return { counts: f.counts };
    } finally { await f.close(); }
  });
  console.log('all native lifecycle controls passed; every localhost server closed');
} finally {
  clearTimeout(deadline);
}
