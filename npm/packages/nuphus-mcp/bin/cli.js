#!/usr/bin/env node
// nuphus-mcp launcher — locates the platform-specific binary in the companion
// nuphus-mcp-<platform>-<arch> package and spawns it as the MCP stdio server.
//
// Mirrors the pattern used by esbuild/@biomejs: the root package is a thin meta
// package whose optionalDependencies install only the binary for the current
// platform. This file is the `bin` entry npm exposes as `nuphus-mcp`.

'use strict';

const path = require('path');
const fs = require('fs');
const { spawn } = require('child_process');

// Map Node's process.platform/arch → our platform package suffix.
function platformPackageName() {
  const platform =
    process.platform === 'win32' ? 'win32' : process.platform === 'darwin' ? 'osx' : process.platform === 'linux' ? 'linux' : process.platform;
  const arch = process.arch === 'x64' ? 'x64' : process.arch === 'arm64' ? 'arm64' : process.arch;
  return `nuphus-mcp-${platform}-${arch}`;
}

function binaryInDir(pkgDir, pkg, binFile) {
  // The platform package always lays the binary at bin/nuphus-mcp(.exe).
  const candidate = path.join(pkgDir, 'bin', binFile);
  if (fs.existsSync(candidate)) return candidate;

  // Fall back to reading the platform package's package.json bin field
  // (covers layout changes in future versions).
  const metaPath = path.join(pkgDir, 'package.json');
  if (fs.existsSync(metaPath)) {
    const meta = JSON.parse(fs.readFileSync(metaPath, 'utf8'));
    if (meta && meta.bin) {
      const binTarget = typeof meta.bin === 'string' ? meta.bin : meta.bin['nuphus-mcp'] || meta.bin[pkg];
      if (binTarget) {
        const p = path.join(pkgDir, binTarget);
        if (fs.existsSync(p)) return p;
      }
    }
  }
  return null;
}

// The meta package's own version (this launcher lives in <meta>/bin).
function metaVersion() {
  try {
    return JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8')).version || null;
  } catch {
    return null;
  }
}

// Version declared by a resolved platform package directory.
function pkgVersionAt(pkgDir) {
  try {
    return JSON.parse(fs.readFileSync(path.join(pkgDir, 'package.json'), 'utf8')).version || null;
  } catch {
    return null;
  }
}

function binaryNameFor(pkg) {
  const binFile = 'nuphus-mcp' + (process.platform === 'win32' ? '.exe' : '');
  const expected = metaVersion();

  // npm can lay out the optional platform packages two ways:
  //   hoisted sibling:  <prefix>/node_modules/@nuphus/<pkg>/bin/<bin>
  //   nested:           <prefix>/node_modules/@nuphus/<meta>/node_modules/@nuphus/<pkg>/bin/<bin>
  // (npm >= 10 tends to NEST optionalDependencies under the dependent package
  // in a global install, so `__dirname/../..` never resolves. Walk up the
  // directory tree checking node_modules at each level, like `require` does.)
  //
  // Version guard: a walk-up hit can land on a STALE platform package left over
  // from an older install — running a mismatched binary silently is worse than
  // failing loudly, so mismatches are collected and only reported when no
  // version-matching candidate exists anywhere up the tree.
  const mismatches = [];
  let dir = __dirname;
  for (;;) {
    for (const sub of [path.join('node_modules', '@nuphus', pkg), path.join('node_modules', pkg)]) {
      const pkgDir = path.join(dir, sub);
      const found = binaryInDir(pkgDir, pkg, binFile);
      if (found) {
        const v = pkgVersionAt(pkgDir);
        if (!expected || !v || v === expected) return found;
        mismatches.push(`${pkgDir} (platform ${v} ≠ meta ${expected})`);
      }
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  if (mismatches.length) {
    console.error(`[nuphus-mcp] found ${pkg} but its version does not match the meta package @${expected}:
  - ${mismatches.join('\n  - ')}

The install is corrupt or version-mismatched. Reinstall to fix:

    npm install -g @nuphus/nuphus-mcp`);
    process.exit(1);
  }
  return null;
}

function run() {
  const pkg = platformPackageName();
  const bin = binaryNameFor(pkg);

  if (!bin) {
    console.error(`[nuphus-mcp] could not locate the ${pkg} binary.

This usually means the platform package was not installed. Install it explicitly:

    npm install -g @nuphus/${pkg}

or reinstall the meta package:

    npm install -g @nuphus/nuphus-mcp`);
    process.exit(1);
  }

  const child = spawn(bin, process.argv.slice(2), {
    stdio: 'inherit',
    windowsHide: true,
  });
  child.on('error', (err) => {
    console.error(`[nuphus-mcp] failed to spawn ${bin}: ${err.message}`);
    process.exit(1);
  });
  child.on('exit', (code, signal) => {
    if (signal) process.kill(process.pid, signal);
    else process.exit(code == null ? 0 : code);
  });
}

run();