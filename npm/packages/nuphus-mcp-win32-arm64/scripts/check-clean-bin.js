#!/usr/bin/env node
// Local-publish guard (prepublishOnly).
//
// The release pipeline (.github/workflows/release.yml) builds the binary per
// platform on CI and copies it into bin/ right before `npm pack`. A MANUAL
// `npm publish` from a developer workstation would silently ship whatever stale
// binary happens to sit in bin/ (15-38MB, gitignored, invisible in diffs) —
// this is exactly how 0.1.4 once shipped a 0.1.5 binary by hand.
//
// Rule: if bin/ contains a compiled binary (>1MB), refuse unless the publisher
// explicitly opts in with NUPHUS_LOCAL_PUBLISH=1 (deliberate manual release).
'use strict';

const fs = require('fs');
const path = require('path');

if (process.env.NUPHUS_LOCAL_PUBLISH === '1') process.exit(0);

const binDir = path.join(__dirname, '..', 'bin');
const MIN_BINARY_BYTES = 1024 * 1024;
let large = [];
try {
  large = fs
    .readdirSync(binDir)
    .filter((f) => f !== '.gitkeep')
    .filter((f) => {
      try {
        return fs.statSync(path.join(binDir, f)).size > MIN_BINARY_BYTES;
      } catch {
        return false;
      }
    });
} catch {
  // No bin dir at all → nothing to guard.
}

if (large.length) {
  console.error('[check-clean-bin] REFUSING local publish: bin/ contains compiled binaries:');
  for (const f of large) console.error(`  - ${f}`);
  console.error('');
  console.error('Binaries are built and packed by .github/workflows/release.yml (tag-driven).');
  console.error('A manual publish from a workstation ships whatever stale binary sits in bin/.');
  console.error('');
  console.error('If this is a deliberate manual release, run:');
  console.error('  NUPHUS_LOCAL_PUBLISH=1 npm publish');
  process.exit(1);
}
