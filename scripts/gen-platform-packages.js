#!/usr/bin/env node
// Generate the 5 platform-package skeletons under npm/packages/nuphus-mcp-<platform>-<arch>.
// Each platform package declares os/cpu and points its bin at the compiled binary.
// The actual binary is copied in by the release workflow (or manually for local dev);
// this script only writes the package.json + a placeholder .gitkeep so the dirs exist.
//
// Platform set mirrors what ORT 1.27 ships in its NuGet all-platform package:
//   win32-x64, win32-arm64, linux-x64, linux-arm64, osx-arm64 (no Intel-mac lib).
'use strict';

const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..');
const OUT_DIR = path.join(ROOT, 'npm', 'packages');

const VERSION = require(path.join(ROOT, 'npm', 'packages', 'nuphus-mcp', 'package.json')).version;

const PLATFORMS = [
  { platform: 'win32', arch: 'x64', exe: 'nuphus-mcp.exe' },
  { platform: 'win32', arch: 'arm64', exe: 'nuphus-mcp.exe' },
  { platform: 'linux', arch: 'x64', exe: 'nuphus-mcp' },
  { platform: 'linux', arch: 'arm64', exe: 'nuphus-mcp' },
  { platform: 'osx', arch: 'arm64', exe: 'nuphus-mcp' },
];

for (const p of PLATFORMS) {
  const pkgName = `nuphus-mcp-${p.platform}-${p.arch}`;
  const dir = path.join(OUT_DIR, pkgName);
  fs.mkdirSync(path.join(dir, 'bin'), { recursive: true });

  const manifest = {
    name: pkgName,
    version: VERSION,
    description: `nuphus-mcp binary for ${p.platform} ${p.arch}. Installed automatically by the nuphus-mcp meta package.`,
    license: 'MIT',
    repository: {
      type: 'git',
      url: 'git+https://github.com/mrpulor-gh/nuphus-mcp.git',
    },
    os: [p.platform],
    cpu: [p.arch],
    bin: {
      'nuphus-mcp': path.join('bin', p.exe),
    },
    files: ['bin'],
  };

  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest, null, 2) + '\n');
  // Placeholder so the dir is non-empty before CI copies the real binary in.
  const gitkeep = path.join(dir, 'bin', '.gitkeep');
  if (!fs.existsSync(gitkeep)) fs.writeFileSync(gitkeep, '');
  console.log(`generated ${pkgName}`);
}

console.log('Done. Copy the compiled binary into each platform package bin/ before npm pack.');
