import { mkdtemp, mkdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';

const baseUrl = (process.env.MINI_CRATES_BASE_URL || 'http://127.0.0.1:3334').replace(/\/$/, '');
const adminToken = process.env.MINI_CRATES_ADMIN_TOKEN || 'dev-bootstrap-admin-token';
const root = await mkdtemp(path.join(tmpdir(), 'mini-crates-e2e-'));
const cargoHome = path.join(root, 'cargo-home');
const registryName = 'liberte';
const crateName = `liberte_crates_smoke_${Date.now().toString(36)}`;

await mkdir(cargoHome, { recursive: true });

try {
  const token = await createToken('cargo smoke token', {
    read: ['liberte_crates_smoke_*'],
    publish: ['liberte_crates_smoke_*']
  });
  await expectWhoami(token);
  await exerciseTokenRotation();
  await writeCargoConfig();
  const betaCrateDir = await createCrate(`${crateName}_beta`, crateName, '0.1.0-beta.1');
  await publishCrate(betaCrateDir, token);
  await consumeCrate('beta-consumer', crateName, '=0.1.0-beta.1', token);
  await yankAndUnyank(crateName, '0.1.0-beta.1', token);

  const stableCrateDir = await createCrate(`${crateName}_stable`, crateName, '0.1.0');
  await publishCrate(stableCrateDir, token);
  await consumeCrate('stable-consumer', crateName, '=0.1.0', token);
  await pathLink(stableCrateDir);
  console.log(`mini-crates cargo smoke passed for ${crateName}`);
} catch (error) {
  console.error(error);
  console.error(`temporary root: ${root}`);
  process.exitCode = 1;
}

async function createToken(name, claims) {
  const response = await request('/api/v1/tokens', {
    method: 'POST',
    token: adminToken,
    body: {
      name,
      claims
    }
  });
  return response.token;
}

async function exerciseTokenRotation() {
  const created = await request('/api/v1/tokens', {
    method: 'POST',
    token: adminToken,
    body: {
      name: 'cargo rotate smoke',
      claims: { read: ['*'], publish: [] }
    }
  });
  const rotated = await request(`/api/v1/tokens/${created.summary.id}/rotate`, {
    method: 'POST',
    token: adminToken
  });
  await expectUnauthorized(created.token);
  await expectWhoami(rotated.token);
  await request(`/api/v1/tokens/${created.summary.id}/revoke`, {
    method: 'POST',
    token: adminToken
  });
  await expectUnauthorized(rotated.token);
}

async function expectWhoami(token) {
  const whoami = await request('/-/whoami', { token });
  if (!whoami.username) {
    throw new Error('whoami response missing username');
  }
}

async function expectUnauthorized(token) {
  const response = await fetch(`${baseUrl}/-/whoami`, {
    headers: { authorization: token }
  });
  if (response.status !== 401) {
    throw new Error(`expected token to be unauthorized, got ${response.status}`);
  }
}

async function writeCargoConfig() {
  await mkdir(path.join(cargoHome, 'registry'), { recursive: true });
  await writeFile(
    path.join(cargoHome, 'config.toml'),
    [
      `[registries.${registryName}]`,
      `index = "sparse+${baseUrl}/api/v1/crates/"`,
      '',
      '[registry]',
      'global-credential-providers = ["cargo:token"]',
      ''
    ].join('\n')
  );
}

async function createCrate(dirname, name, version) {
  const crateDir = path.join(root, dirname);
  await mkdir(path.join(crateDir, 'src'), { recursive: true });
  await writeFile(
    path.join(crateDir, 'Cargo.toml'),
    [
      '[package]',
      `name = "${name}"`,
      `version = "${version}"`,
      'edition = "2021"',
      'description = "mini crates smoke crate"',
      'license = "MIT"',
      `publish = ["${registryName}"]`,
      '',
      '[lib]',
      'path = "src/lib.rs"',
      ''
    ].join('\n')
  );
  await writeFile(
    path.join(crateDir, 'src/lib.rs'),
    `pub fn value() -> &'static str { "${name}@${version}" }\n`
  );
  return crateDir;
}

async function publishCrate(cwd, token) {
  await run('cargo', ['publish', '--registry', registryName, '--allow-dirty', '--no-verify'], {
    cwd,
    token
  });
}

async function consumeCrate(dirname, name, version, token) {
  const consumerDir = path.join(root, dirname);
  await mkdir(path.join(consumerDir, 'src'), { recursive: true });
  await writeFile(
    path.join(consumerDir, 'Cargo.toml'),
    [
      '[package]',
      `name = "${dirname.replaceAll('-', '_')}"`,
      'version = "0.1.0"',
      'edition = "2021"',
      '',
      '[dependencies]',
      `${name} = { version = "${version}", registry = "${registryName}" }`,
      ''
    ].join('\n')
  );
  await writeFile(path.join(consumerDir, 'src/main.rs'), `fn main() { println!("{}", ${name}::value()); }\n`);
  await run('cargo', ['check'], { cwd: consumerDir, token });
}

async function yankAndUnyank(name, version, token) {
  await run('cargo', ['yank', '--registry', registryName, '--version', version, name], { cwd: root, token });
  await run('cargo', ['yank', '--registry', registryName, '--version', version, '--undo', name], {
    cwd: root,
    token
  });
}

async function pathLink(crateDir) {
  const consumerDir = path.join(root, 'path-link-consumer');
  await mkdir(path.join(consumerDir, 'src'), { recursive: true });
  await writeFile(
    path.join(consumerDir, 'Cargo.toml'),
    [
      '[package]',
      'name = "path_link_consumer"',
      'version = "0.1.0"',
      'edition = "2021"',
      '',
      '[dependencies]',
      `${crateName} = { path = ${JSON.stringify(crateDir)} }`,
      ''
    ].join('\n')
  );
  await writeFile(path.join(consumerDir, 'src/main.rs'), `fn main() { println!("{}", ${crateName}::value()); }\n`);
  await run('cargo', ['check'], { cwd: consumerDir });
}

async function request(route, { method = 'GET', token, body } = {}) {
  const response = await fetch(`${baseUrl}${route}`, {
    method,
    headers: {
      authorization: token,
      ...(body ? { 'content-type': 'application/json' } : {})
    },
    body: body ? JSON.stringify(body) : undefined
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`${method} ${route} failed ${response.status}: ${text}`);
  }
  return text ? JSON.parse(text) : {};
}

async function run(command, args, { cwd, token } = {}) {
  const output = await runCapture(command, args, { cwd, token });
  if (process.env.MINI_CRATES_E2E_VERBOSE === '1') {
    process.stdout.write(output);
  }
}

async function runCapture(command, args, { cwd, token } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: {
        ...process.env,
        CARGO_HOME: cargoHome,
        CARGO_TERM_COLOR: 'never',
        ...(token ? { CARGO_REGISTRIES_LIBERTE_TOKEN: token } : {})
      },
      stdio: ['ignore', 'pipe', 'pipe']
    });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', chunk => {
      stdout += chunk;
    });
    child.stderr.on('data', chunk => {
      stderr += chunk;
    });
    child.on('error', reject);
    child.on('close', code => {
      const output = `${stdout}${stderr}`;
      if (code === 0) {
        resolve(output);
        return;
      }
      reject(new Error(`${command} ${args.join(' ')} failed with ${code}\n${output}`));
    });
  });
}
