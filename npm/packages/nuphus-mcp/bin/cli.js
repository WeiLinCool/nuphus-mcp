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

function binaryNameFor(pkg) {
  // The platform package always lays the binary at bin/nuphus-mcp(.exe).
  const pkgDir = path.join(__dirname, '..', '..', pkg);
  const binFile = 'nuphus-mcp' + (process.platform === 'win32' ? '.exe' : '');
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
