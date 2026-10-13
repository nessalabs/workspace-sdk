// Run with the unpacked @boatdev/sdk@1.9.7 directory as argv[2].
// Runs the published request builders and converters without network access.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const packagePath = path.resolve(process.argv[2] || 'node_modules/@boatdev/sdk');
const sdk = require(packagePath);
assert.equal(require(path.join(packagePath, 'package.json')).version, '1.9.7');
const fixtures = JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures/runtime.json'), 'utf8'));
const models = {
  prompt: 'PromptResponse', promptRun: 'PromptRunResponse', events: 'EventsResponse',
  environments: 'SandboxEnvironmentListResponse', environment: 'SandboxEnvironmentResponse',
  deletion: 'DeletionOperationResponse', snapshots: 'SnapshotListResponse', latest: 'SnapshotLatestResponse',
  snapshotTree: 'SnapshotTreeResponse', snapshotDownload: 'SnapshotDownloadResponse',
  commandStarted: 'CommandStartedResponse', commandStatus: 'CommandStatusResponse', desktop: 'DesktopResponse',
};
function assertProjection(actual, original, name) {
  if (Array.isArray(actual)) {
    assert.equal(actual.length, original.length, name);
    actual.forEach((value, index) => assertProjection(value, original[index], `${name}[${index}]`));
  } else if (actual && typeof actual === 'object') {
    for (const [key, value] of Object.entries(actual)) assertProjection(value, original[key], `${name}.${key}`);
  } else {
    assert.deepEqual(actual, original, name);
  }
}
for (const [fixture, model] of Object.entries(models)) {
  const parsed = sdk[`${model}FromJSON`](fixtures[fixture]);
  assert.ok(sdk[`instanceOf${model}`](parsed), `${model} required fields missing`);
  // Dates normalize back to ISO strings; JSON drops optional undefined fields.
  const roundTrip = JSON.parse(JSON.stringify(sdk[`${model}ToJSON`](parsed)));
  // Rust preserves additional extension fields; every field retained by the
  // published SDK, including nested arrays and objects, must match the fixture.
  assertProjection(roundTrip, fixtures[fixture], model);
}
assert.equal(sdk.SandboxEventFromJSON(fixtures.events.events[0]).id, undefined);
assert.equal(sdk.SandboxEnvironmentFromJSON(fixtures.environments.environments[0]).selectedRepositories[0]._private, true);
assert.deepEqual(sdk.PromptRequestToJSON({provider:'codex',prompt:'do it'}),
  {provider:'codex',model:undefined,reasoningEffort:undefined,fast:undefined,new:undefined,conversationId:undefined,prompt:'do it'});
(async () => {
  const api = new sdk.BoatApi(new sdk.Configuration({accessToken:'fixture-only'}));
  const id = '11111111-2222-3333-4444-555555555555';
  assert.equal((await api.updateEnvironmentRequestOpts({environmentId:id,updateSandboxEnvironmentRequest:{}})).path, `/environments/${id}`);
  const q = await api.listSandboxSnapshotsRequestOpts({sandboxId:'bx_23456789',limit:1,cursor:'cursor/+=',sort:'asc'});
  assert.deepEqual(q.query,{limit:1,cursor:'cursor/+=',sort:'asc'});
  const command = await api.commandStatusRequestOpts({sandboxId:'bx_23456789',processId:123,tailBytes:4096});
  assert.equal(command.query.tailBytes,4096);
  const file = await api.getSnapshotFileRequestOpts({snapshotId:'snap_test-1',path:'/home/user/a b?#.bin'});
  assert.equal(file.path,'/snapshots/snap_test-1/files');
  assert.equal(file.query.path,'/home/user/a b?#.bin');
  const tree = await api.getSnapshotTreeRequestOpts({snapshotId:'snap_test-1'});
  assert.deepEqual(tree.query,{});
  console.log('PASS: 13 runtime fixtures and request contracts match @boatdev/sdk@1.9.7');
})().catch(error => { console.error(error); process.exitCode=1; });
