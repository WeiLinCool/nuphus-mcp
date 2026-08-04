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
const binFile = `nuphus-mcp${process.platform === 'win32' ? '.exe' : ''}`;

// Same walk-up search as cli.js: npm >= 10 nests optionalDependencies under
// the dependent package in global installs instead of hoisting them as
// siblings, so both layouts must be checked.
//
// Version guard (mirrors cli.js): a walk-up hit on a STALE platform package
// from an older meta version must fail loudly, not pass the check.
function pkgVersionAt(pkgDir) {
  try {
    return JSON.parse(fs.readFileSync(path.join(pkgDir, 'package.json'), 'utf8')).version || null;
  } catch {
    return null;
  }
}
let metaV = null;
try {
  metaV = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'package.json'), 'utf8')).version || null;
} catch {
  metaV = null;
}

let exe = null;
const mismatches = [];
let dir = __dirname;
for (;;) {
  for (const sub of [path.join('node_modules', '@nuphus', pkg), path.join('node_modules', pkg)]) {
    const pkgDir = path.join(dir, sub);
    const candidate = path.join(pkgDir, 'bin', binFile);
    if (fs.existsSync(candidate)) {
      const v = pkgVersionAt(pkgDir);
      if (!metaV || !v || v === metaV) {
        exe = candidate;
        break;
      }
      mismatches.push(`${pkgDir} (platform ${v} ≠ meta ${metaV})`);
    }
  }
  if (exe) break;
  const parent = path.dirname(dir);
  if (parent === dir) break;
  dir = parent;
}

if (exe) {
  process.exit(0); // good
}

if (mismatches.length) {
  console.error(`[nuphus-mcp] platform package '${pkg}' was found but its version does not match the meta package @${metaV}:`);
  for (const m of mismatches) console.error(`  - ${m}`);
  console.error('');
  console.error('The install is corrupt or version-mismatched. Reinstall to fix:');
  console.error('');
  console.error('    npm install -g @nuphus/nuphus-mcp');
  process.exit(1);
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