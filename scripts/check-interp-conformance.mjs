#!/usr/bin/env node
// Checks the Rust tree-walking interpreter (compiler/crates/mrt-interp) against
// the Python reference implementation over the shared conformance corpus in
// scripts/parity-cases.mjs -- the same corpus the Playground's TypeScript
// interpreter is held to.
//
// A case that stops on an unimplemented feature is recorded as *unsupported*
// rather than as a mismatch. That would be a hole big enough to hide a
// regression in, so the unsupported set is a ratchet: it is compared against
// UNSUPPORTED below, and the harness fails both when a case newly stops
// working and when a listed case starts working. Implementing a feature is
// therefore expected to make this script fail once, on purpose, until the list
// is shortened.
//
// That list is now empty -- the Rust interpreter runs everything the reference
// does -- which is exactly when a ratchet earns its keep: there is nowhere for
// a regression to hide as "not implemented".
//
// Run with `npm run check:rust-interp`.

import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync, mkdtempSync, writeFileSync, mkdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { REGRESSION_CASES, MODULE_CASES, ENGINE_SPECIFIC_EXAMPLES } from './parity-cases.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(__dirname, '..')

// Cases the Rust interpreter is known not to run yet. Keep the reason with
// the name: a bare list of names decays into a list of things nobody
// remembers being broken.
// Empty, and that is the news: the Rust interpreter now runs every construct
// the reference does. Generators were the last gap, and they closed by
// compiling a generator body and parking it as a VM frame rather than by
// finding a coroutine for the tree-walker to borrow -- there is none in stable
// Rust to borrow. Keep the ratchet: a feature that stops working should fail
// here rather than quietly join a list.
const UNSUPPORTED = new Set([])

const NOT_IMPLEMENTED = /are not implemented in this interpreter yet/

function run(cmd, args, options = {}) {
  try {
    const stdout = execFileSync(cmd, args, {
      cwd: repoRoot,
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'pipe'],
      ...options,
    })
    return stdout.trimEnd()
  } catch (err) {
    // A program that stops on an error exits non-zero and writes the message
    // to stderr. That text is part of the language's behaviour, so both sides
    // are compared on stdout+stderr rather than only on success.
    if (err.stdout === undefined && err.stderr === undefined) throw err
    return `${err.stdout ?? ''}${err.stderr ?? ''}`.trimEnd()
  }
}

const binary = path.join(repoRoot, 'compiler/target/release/mrt-run')

function runPythonFile(file) {
  return run('python3', ['-m', 'mrt', file])
}

function runRustFile(file) {
  return run(binary, [file])
}

function main() {
  process.stderr.write('building mrt-run...\n')
  execFileSync('cargo', ['build', '--release', '--bin', 'mrt-run'], {
    cwd: path.join(repoRoot, 'compiler'),
    stdio: ['ignore', 'ignore', 'inherit'],
  })

  const scratch = mkdtempSync(path.join(tmpdir(), 'mrt-rust-'))
  let checked = 0
  let failures = 0
  const unsupported = new Set()

  function report(name, expected, actual) {
    if (NOT_IMPLEMENTED.test(actual)) {
      unsupported.add(name)
      console.log(`- ${name} (not implemented yet)`)
      return
    }
    checked++
    if (expected === actual) {
      console.log(`✓ ${name}`)
      return
    }
    failures++
    console.error(`\n✗ MISMATCH: ${name}`)
    console.error('  python:', JSON.stringify(expected))
    console.error('  rust:  ', JSON.stringify(actual))
  }

  function checkFile(name, file) {
    report(name, runPythonFile(file), runRustFile(file))
  }

  function checkSource(name, src) {
    const safe = name.replace(/[^a-z0-9]+/gi, '_').slice(0, 60)
    const file = path.join(scratch, `${safe}.mrt`)
    writeFileSync(file, src)
    checkFile(name, file)
  }

  function checkFiles(name, files) {
    const safe = name.replace(/[^a-z0-9]+/gi, '_').slice(0, 60)
    const root = path.join(scratch, `mod_${safe}`)
    for (const [relative, contents] of Object.entries(files)) {
      const full = path.join(root, relative)
      mkdirSync(path.dirname(full), { recursive: true })
      writeFileSync(full, contents)
    }
    checkFile(name, path.join(root, 'main.mrt'))
  }

  const examplesDir = path.join(repoRoot, 'examples')
  for (const file of readdirSync(examplesDir).sort()) {
    if (!file.endsWith('.mrt')) continue
    if (ENGINE_SPECIFIC_EXAMPLES.has(file)) continue
    checkFile(`example: ${file}`, path.join(examplesDir, file))
  }
  for (const { name, src } of REGRESSION_CASES) {
    checkSource(`regression: ${name}`, src)
  }
  for (const { name, files } of MODULE_CASES) {
    checkFiles(`modules: ${name}`, files)
  }

  console.log(`\n${checked - failures}/${checked} checks matched (${unsupported.size} not implemented yet).`)

  const appeared = [...unsupported].filter((n) => !UNSUPPORTED.has(n))
  const disappeared = [...UNSUPPORTED].filter((n) => !unsupported.has(n))
  if (appeared.length) {
    console.error(`\nThese cases newly stop on an unimplemented feature:\n  ${appeared.join('\n  ')}`)
  }
  if (disappeared.length) {
    console.error(
      `\nThese cases now run and should be removed from UNSUPPORTED in ${path.basename(__filename)}:\n  ${disappeared.join('\n  ')}`,
    )
  }
  if (failures || appeared.length || disappeared.length) process.exit(1)
}

const __filename = fileURLToPath(import.meta.url)
main()
