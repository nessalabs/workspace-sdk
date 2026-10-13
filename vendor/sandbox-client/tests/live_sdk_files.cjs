// Used by the ignored Rust live file round-trip test. Credentials stay in env.
const assert = require('node:assert/strict');
const path = require('node:path');
const sdk = require(path.resolve(process.env.BOAT_SDK_PATH));
const api = new sdk.BoatApi(new sdk.Configuration({
  accessToken: process.env.BOAT_API_KEY,
  basePath: process.env.BOAT_BASE_URL || 'https://boat.dev/api/v1',
}));
(async () => {
  const sandboxId = process.env.SANDBOX_ID;
  const filePath = process.env.BOAT_TEST_PATH;
  const read = await api.readFile({ sandboxId, path: filePath, encoding: 'utf8' }, { signal: AbortSignal.timeout(60_000) });
  assert.equal(read.content, 'written-by-rust');
  await api.writeFile({ sandboxId, fileWriteRequest: {
    path: filePath, content: 'written-by-typescript', encoding: 'utf8',
  } }, { signal: AbortSignal.timeout(60_000) });
  console.log('PASS: SDK read Rust file and wrote replacement');
})().catch(error => {
  console.error(`SDK file round trip failed (HTTP ${error.response?.status || 'unknown'})`);
  process.exitCode = 1;
});
