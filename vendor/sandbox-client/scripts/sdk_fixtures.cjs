// Generate/check fixtures using the actual published SDK; never makes API calls.
// node scripts/sdk_fixtures.cjs /path/to/sdk/package [--write]
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const pkg = path.resolve(process.argv[2]);
const sdk = require(pkg);
assert.equal(require(path.join(pkg, 'package.json')).version, '1.9.7');
const apiSource = fs.readFileSync(path.join(pkg, 'src/apis/BoatApi.ts'), 'utf8');
const models = path.join(pkg, 'src/models');

function fields(source, name) {
  const start = source.indexOf(`export interface ${name} {`);
  if (start < 0) return [];
  const block = source.slice(start, source.indexOf('\n}', start));
  return [...block.matchAll(/^    (\w+)(\?)?: (.+);$/gm)]
    .map(([, name, optional, type]) => ({ name, optional: !!optional, type }));
}

function scalar(type, name, source, depth = 0) {
  type = type.replace(/ \| null$/, '');
  if (type === 'string') {
    if (name === 'sandboxId' || name === 'sourceSandboxId') return 'bx_23456789';
    if (name === 'url' || name === 'signedUrl') return 'https://example.test/path?token=fixture-secret';
    if (name === 'path') return '/home/user/fixture a?#.txt';
    if (name === 'key') return 'KEY_WITH-HYPHEN';
    if (name === 'keyPrefix' || name === 'keyLastFour') return 'fake';
    if (name === 'id') return 'id_with-hyphen';
    if (name === 'cursor') return 'cursor/+=';
    if (name === 'encoding') return 'utf8';
    return `${name}_with-hyphen`;
  }
  if (type === 'number') return name === 'port' ? 8080 : name === 'timestamp' ? 1000 : 2;
  // This surface supports JSON commands; detached/NDJSON cases have separate real boundary tests.
  if (type === 'boolean') return name !== 'detached' && name !== 'stream';
  if (type === 'Date') return '2026-09-07T00:00:00.000Z';
  if (type.startsWith('Array<') || type.startsWith('Set<')) {
    return [scalar(type.slice(type.indexOf('<') + 1, -1), name, source, depth + 1)];
  }
  if (type.startsWith('{')) return { fixture: 'fixture-secret' };
  if (type.endsWith('Enum')) {
    const match = source.match(new RegExp(`export const ${type} = \\{\\s*\\w+: ([^,\\n]+)`));
    if (!match) throw Error(`missing enum ${type}`);
    const value = match[1].trim();
    return value.startsWith("'") ? value.slice(1, -1) : Number(value);
  }
  if (type === 'WebhookEventType') return 'box.ready';
  return model(type, depth + 1);
}

function model(name, depth = 0) {
  if (depth > 15) throw Error(`recursive model ${name}`);
  if (name === 'Command200Response') name = 'CommandResponse';
  const source = fs.readFileSync(path.join(models, `${name}.ts`), 'utf8');
  const value = {};
  for (const field of fields(source, name)) {
    const wire = field.name.startsWith('_') ? field.name.slice(1) : field.name;
    value[wire] = scalar(field.type, field.name, source, depth);
  }
  if (name === 'Sandbox') Object.assign(value, { id: 'bx_23456789', state: 'idle' });
  if (name === 'SandboxEvent') Object.assign(value, {
    id: 'event_with-hyphen', type: 'response', taskId: 'prompt_with-hyphen',
  });
  return value;
}

function minimalModel(name, raw) {
  if (name === 'Command200Response') name = 'CommandResponse';
  const source = fs.readFileSync(path.join(models, `${name}.ts`), 'utf8');
  const result = {};
  for (const field of fields(source, name)) {
    if (field.optional) continue;
    const wire = field.name.startsWith('_') ? field.name.slice(1) : field.name;
    const type = field.type;
    if (type.endsWith(' | null')) {
      result[wire] = null;
      continue;
    }
    const itemType = type.match(/^(?:Array|Set)<(.+)>$/)?.[1];
    const nested = itemType || type;
    const isModel = fs.existsSync(path.join(models, `${nested}.ts`)) &&
      !nested.endsWith('Enum') && nested !== 'WebhookEventType';
    result[wire] = isModel
      ? (itemType ? raw[wire].map(v => minimalModel(nested, v)) : minimalModel(nested, raw[wire]))
      : raw[wire];
  }
  return result;
}

function runtimeParameters(parameters, definitions) {
  const result = { ...parameters };
  for (const field of definitions) {
    const converter = sdk[`${field.type}FromJSON`];
    if (converter && field.name in parameters) result[field.name] = converter(parameters[field.name]);
  }
  return result;
}

function json(value) { return JSON.parse(JSON.stringify(value)); }

(async () => {
  const api = new sdk.BoatApi(new sdk.Configuration({ accessToken: 'fixture-only' }));
  const cases = [];
  const supported = require('./supported-operations.json');
  const rx = /    async (\w+)RequestOpts\((.*?)\): Promise<runtime.RequestOpts> \{([\s\S]*?)\n    \}/g;
  for (const match of apiSource.matchAll(rx)) {
    const [, operation, args] = match;
    if (!supported.includes(operation)) continue;
    const iface = args.match(/(?:requestParameters|input): (\w+)/)?.[1];
    const definitions = iface ? fields(apiSource, iface) : [];
    const parameters = {};
    for (const field of definitions) parameters[field.name] = scalar(field.type, field.name, apiSource);
    // The Rust convenience methods send the identifier as deletion confirmation.
    if ('xAsciiConfirmDelete' in parameters) parameters.xAsciiConfirmDelete = parameters.sandboxId || parameters.snapshotId;
    const minimalParameters = {};
    for (const field of definitions) {
      if (!field.optional) minimalParameters[field.name] = sdk[`${field.type}FromJSON`]
        ? minimalModel(field.type, parameters[field.name]) : parameters[field.name];
    }
    const request = await api[`${operation}RequestOpts`](...(iface ? [runtimeParameters(parameters, definitions)] : []));
    const minimalRequest = await api[`${operation}RequestOpts`](...(iface ? [runtimeParameters(minimalParameters, definitions)] : []));
    const rest = apiSource.slice(match.index + match[0].length);
    const responseModel = rest.match(new RegExp(`async ${operation}Raw\\([\\s\\S]*?Promise<runtime.ApiResponse<(.*?)>>`))[1];
    const raw = responseModel === 'Blob' ? null : model(responseModel);
    const expected = responseModel === 'Blob' ? null : json(sdk[`${responseModel}ToJSON`](sdk[`${responseModel}FromJSON`](raw)));
    cases.push({
      operation, parameters, request: json(request), responseModel, response: raw,
      expectedResponse: expected,
      minimalResponse: responseModel === 'Blob' ? null : minimalModel(responseModel, raw),
      minimalParameters, minimalRequest: json(minimalRequest),
    });
  }
  assert.equal(cases.length, 59);
  assert.deepEqual([...cases.map(c => c.operation)].sort(), [...supported].sort());
  // FromJSON omits nulls, but callers can construct nullable request options directly.
  const promptNull = await api.promptRequestOpts({ sandboxId: 'bx_23456789', promptRequest: {
    provider: 'codex', prompt: 'fixture prompt', model: null, reasoningEffort: null,
  } });
  assert.deepEqual(promptNull.body, { provider: 'codex', prompt: 'fixture prompt', model: null, reasoningEffort: null, fast: undefined, conversationId: undefined, new: undefined });
  const result = { sdkVersion: '1.9.7', cases };
  const target = path.join(__dirname, '../tests/fixtures/sdk_operations.json');
  if (process.argv.includes('--write')) fs.writeFileSync(target, JSON.stringify(result, null, 2) + '\n');
  else assert.deepEqual(JSON.parse(fs.readFileSync(target, 'utf8')), result, 'SDK operation contracts drifted');
  console.log(`PASS: ${cases.length} real SDK operation contracts ${process.argv.includes('--write') ? 'generated' : 'verified'}`);
})().catch(error => { console.error(error); process.exitCode = 1; });
