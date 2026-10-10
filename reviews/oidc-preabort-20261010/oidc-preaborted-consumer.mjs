import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { setTimeout } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';

// Import the built public package root, including its real SDK/error modules.
const { clientCredentials } = await import(pathToFileURL(process.argv[2]).href);
let discoveryRequests = 0;
const server = createServer((_req, res) => {
  discoveryRequests += 1;
  res.writeHead(503, { 'content-type': 'text/plain' });
  res.end('synthetic unavailable discovery');
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const { port } = server.address();
try {
  const provider = clientCredentials({
    issuer: `http://127.0.0.1:${port}`,
    clientId: 'synthetic-client',
    clientSecret: 'synthetic-secret',
    timeoutMs: 100,
  });
  await assert.rejects(provider.getToken(AbortSignal.abort()), { code: 'canceled' });
  console.log('caught expected canceled result');
  // Give a mistakenly-started real request time to receive the local 503. The
  // strict Node rejection mode makes any orphaned exchange terminate the child.
  await setTimeout(250);
  assert.equal(discoveryRequests, 0, 'pre-aborted token request must not start discovery');
  console.log('consumer survived; discoveryRequests=0');
} finally {
  server.closeAllConnections();
  await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
}
