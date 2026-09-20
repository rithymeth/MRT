// Package every multi-module program we have, and insist the packaged file
// behaves exactly like the original.
//
// This is the only check on `mrt-game package`, and it is the only kind worth
// having: a packager is correct when the thing it produces is
// indistinguishable from the thing it consumed. Anything less -- "it parses",
// "it has the right number of lines" -- would pass for a packager that
// silently dropped a module.
//
// The corpus already contains the awkward cases (aliasing, namespace imports,
// re-export chains, a module imported twice, cycles), so they are reused
// rather than invented again here.

import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, writeFileSync, rmSync, readdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { MODULE_CASES } from './parity-cases.mjs'

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const binDir = path.join(repoRoot, 'compiler', 'target', 'release')
const mrtRun = path.join(binDir, 'mrt-run')
const mrtGame = path.join(binDir, 'mrt-game')

function run(cmd, args) {
  try {
    return { out: execFileSync(cmd, args, { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }).trimEnd(), ok: true }
  } catch (e) {
    return { out: `${e.stdout ?? ''}${e.stderr ?? ''}`.trimEnd(), ok: false }
  }
}

let checked = 0
let failures = 0

function report(name, detail) {
  failures += 1
  console.error(`✗ ${name}`)
  console.error(`  ${detail.replace(/\n/g, '\n  ')}`)
}

function check(name, files) {
  checked += 1
  const root = mkdtempSync(path.join(tmpdir(), 'mrtpkg-'))
  try {
    for (const [relative, contents] of Object.entries(files)) {
      const full = path.join(root, relative)
      mkdirSync(path.dirname(full), { recursive: true })
      writeFileSync(full, contents)
    }
    const entry = path.join(root, 'main.mrt')
    const before = run(mrtRun, [entry])

    const packaged = run(mrtGame, ['package', entry])
    if (!packaged.ok) {
      // A program that does not run cannot be expected to package -- a cycle
      // is refused by both, which is the right answer from each. The reason
      // is printed rather than swallowed: a bare "refused" is how a packager
      // regression hides inside a green check.
      if (!before.ok) {
        const why = packaged.out.replace(/^mrt-game: /, '').split('\n')[0]
        console.log(`✓ ${name}`)
        console.log(`    refused: ${why}`)
        return
      }
      report(name, `package refused a program that runs:\n${packaged.out}`)
      return
    }

    const after = run(mrtRun, [path.join(root, 'game.packaged.mrt')])
    if (after.out !== before.out) {
      report(name, `original:\n${before.out}\npackaged:\n${after.out}`)
      return
    }
    // And on the other engine, since both have to run what ships.
    const vm = run(mrtRun, ['--vm', path.join(root, 'game.packaged.mrt')])
    if (vm.out !== before.out) {
      report(name, `the VM disagrees:\n${before.out}\nvs\n${vm.out}`)
      return
    }
    console.log(`✓ ${name}`)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
}

console.error('building mrt-run and mrt-game...')
execFileSync('cargo', ['build', '--release', '--bin', 'mrt-run', '--bin', 'mrt-game'], {
  cwd: path.join(repoRoot, 'compiler'),
  stdio: ['ignore', 'ignore', 'inherit'],
})

for (const { name, files } of MODULE_CASES) {
  check(`modules: ${name}`, files)
}

// The examples that actually import something, packaged in place.
for (const file of readdirSync(path.join(repoRoot, 'examples')).sort()) {
  if (!file.endsWith('.mrt')) continue
  const source = path.join(repoRoot, 'examples', file)
  const text = execFileSync('cat', [source], { encoding: 'utf8' })
  if (!/^\s*(import|export)\b/m.test(text)) continue
  checked += 1
  const root = mkdtempSync(path.join(tmpdir(), 'mrtpkg-'))
  try {
    execFileSync('cp', ['-r', path.join(repoRoot, 'examples') + '/.', root])
    const entry = path.join(root, file)
    const before = run(mrtRun, [entry])
    const packaged = run(mrtGame, ['package', entry])
    if (!packaged.ok) {
      report(`example: ${file}`, `package failed:\n${packaged.out}`)
    } else {
      const after = run(mrtRun, [path.join(root, 'game.packaged.mrt')])
      if (after.out !== before.out) {
        report(`example: ${file}`, `original:\n${before.out}\npackaged:\n${after.out}`)
      } else {
        console.log(`✓ example: ${file}`)
      }
    }
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
}

console.log(`\n${checked - failures}/${checked} packaged programs behaved identically.`)
process.exit(failures === 0 ? 0 : 1)
