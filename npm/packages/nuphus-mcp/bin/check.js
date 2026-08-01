#!/usr/bin/env node
// postinstall check — verifies the platform-specific companion package landed.
// If optionalDependencies resolution failed (e.g. npm without --os or a very
// old npm that skips platform-matched optionals), fail loudly with a clear fix.
'use strict';

const path = require('path');
const fs = require('fs');

const platform =
  process.platform === 'win32' ? 'win32' : process.platform === 'darwin' ? 'osx' : process.platform === 'linux' ? 'linux' : process.platform;
const arch = process.arch === 'x64' ? 'x64' : process.arch === 'arm64' ? 'arm64' : process.arch;
const pkg = `nuphus-mcp-${platform}-${arch}`;

const pkgDir = path.join(__dirname, '..', '..', pkg);
const exe = path.join(pkgDir, 'bin', `nuphus-mcp${process.platform === 'win32' ? '.exe' : ''}`);

if (fs.existsSync(exe)) {
  process.exit(0); // good
}

// Optional deps silently vanish on failure; check the meta package.json too
// to give a targeted error rather than a confusing runtime failure later.
console.error(`[nuphus-mcp] platform package '${pkg}' was not installed (binary missing at ${exe}).`);
console.error('');
console.error('This usually happens when npm did not resolve platform-matched optionalDependencies.');
console.error('Fix by installing the platform package explicitly:');
console.error('');
console.error(`    npm install -g @nuphus/${pkg}`);
console.error('');
console.error('Or reinstall with a fresh lockfile:');
console.error('');
console.error('    npm install -g @nuphus/nuphus-mcp --force');
console.error('');
console.error('If this is the active MCP environment, run `/mcp` to reload the server after fixing.');
process.exit(1);
