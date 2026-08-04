#!/usr/bin/env node
// Release version consistency preflight — the FIRST gate of the release pipeline.
//
// Four version sources must agree before anything is built or published:
//   1. the git tag being released (vX.Y.Z)          — release mode only
//   2. the meta package     npm/packages/nuphus-mcp/package.json
//   3. the 5 platform pkgs  npm/packages/nuphus-mcp-<platform>-<arch>/package.json
//   4. the crate version    crates/nuphus-mcp/Cargo.toml (what `initialize` reports)
//
// History: Cargo.toml sat at 0.1.0 while npm/tags moved to 0.1.6, so every
// published binary mis-reported its version; and gen-platform-packages.js had
// to be run BY HAND or the meta package referenced platform packages that did
// not exist on npm ("ghost release").
//
// Modes:
//   node scripts/verify-release-versions.js                    (release mode, needs --tag or TAG_REF)
//   node scripts/verify-release-versions.js --tag v0.1.7
//   node scripts/verify-release-versions.js --check-workspace  (local/CI dry-run: repo sources only, no tag)
'use strict';

const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');
const META_PKG = path.join(ROOT, 'npm', 'packages', 'nuphus-mcp', 'package.json');
const CARGO_TOML = path.join(ROOT, 'crates', 'nuphus-mcp', 'Cargo.toml');
const PLATFORM_DIRS = [
  'nuphus-mcp-win32-x64',
  'nuphus-mcp-win32-arm64',
  'nuphus-mcp-linux-x64',
  'nuphus-mcp-linux-arm64',
  'nuphus-mcp-osx-arm64',
];

function fail(msg) {
  console.error(`[verify-release-versions] FAIL: ${msg}`);
  process.exit(1);
}

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (e) {
    fail(`cannot read ${path.relative(ROOT, file)}: ${e.message}`);
  }
}

function cargoVersion() {
  const toml = fs.readFileSync(CARGO_TOML, 'utf8');
  // The [package] version — first `version = "..."` at line start in the file's
  // [package] section (dependency versions are indented or in later sections).
  const pkgSection = toml.split(/^\[/m).find((s) => s.startsWith('package]'));
  if (!pkgSection) fail('crates/nuphus-mcp/Cargo.toml has no [package] section');
  const m = pkgSection.match(/^version\s*=\s*"([^"]+)"/m);
  if (!m) fail('crates/nuphus-mcp/Cargo.toml [package] has no version field');
  return m[1];
}

function parseArgs() {
  const args = process.argv.slice(2);
  let tag = process.env.TAG_REF || process.env.GITHUB_REF_NAME || null;
  let workspaceOnly = false;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--check-workspace') workspaceOnly = true;
    else if (args[i] === '--tag') tag = args[++i];
    else fail(`unknown argument: ${args[i]}`);
  }
  return { tag, workspaceOnly };
}

function main() {
  const { tag, workspaceOnly } = parseArgs();
  const sources = [];

  // 1. git tag (release mode only)
  if (!workspaceOnly) {
    if (!tag) {
      fail('release mode requires --tag vX.Y.Z (or TAG_REF/GITHUB_REF_NAME); use --check-workspace for repo-only checks');
    }
    const m = String(tag).match(/^v?(\d+\.\d+\.\d+)$/);
    if (!m) fail(`tag '${tag}' is not a vX.Y.Z semver tag`);
    sources.push({ name: `git tag (${tag})`, version: m[1] });
  }

  // 2. meta package
  const meta = readJson(META_PKG);
  sources.push({ name: 'meta package.json', version: meta.version });

  // 3. platform packages (+ optionalDependencies pins must match the meta version)
  for (const dir of PLATFORM_DIRS) {
    const pkg = readJson(path.join(ROOT, 'npm', 'packages', dir, 'package.json'));
    sources.push({ name: `${dir}/package.json`, version: pkg.version });
    const pinned = meta.optionalDependencies && meta.optionalDependencies[pkg.name];
    if (pinned !== meta.version) {
      fail(
        `meta optionalDependencies["${pkg.name}"] is '${pinned}' but meta version is '${meta.version}' — ` +
          `run scripts/gen-platform-packages.js and fix the meta manifest`
      );
    }
  }

  // 4. crate version
  sources.push({ name: 'crates/nuphus-mcp/Cargo.toml', version: cargoVersion() });

  // Agreement check
  const expected = sources[0].version;
  let ok = true;
  for (const s of sources) {
    const mark = s.version === expected ? 'OK ' : 'MISMATCH';
    if (s.version !== expected) ok = false;
    console.log(`  [${mark}] ${s.version.padEnd(10)} ${s.name}`);
  }
  if (!ok) {
    fail(
      `version sources disagree (expected all == ${expected}). ` +
        `Bump all of: git tag, npm/packages/*/package.json (via scripts/gen-platform-packages.js), crates/nuphus-mcp/Cargo.toml`
    );
  }
  console.log(`[verify-release-versions] OK: all ${sources.length} version sources agree on ${expected}`);
}

main();
