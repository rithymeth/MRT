#!/usr/bin/env node
// Checks the Rust tree-walking interpreter (compiler/crates/mrt-interp) against
// the Python reference implementation over the shared conformance corpus in
// scripts/parity-cases.mjs -- the same corpus the Playground's TypeScript
// interpreter is held to.
//
// The Rust interpreter is younger than the other two and does not implement
// generators or modules yet, so a case that stops on one of those is recorded
// as *unsupported* rather than as a mismatch. That would be a hole big enough
// to hide a regression in, so the unsupported set is a ratchet: it is compared
// against UNSUPPORTED below, and the harness fails both when a case newly
// stops working and when a listed case starts working. Implementing a feature
// is therefore expected to make this script fail once, on purpose, until the
// list is shortened.
//
// Run with `npm run check:rust-interp`.

import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync, mkdtempSync, writeFileSync, mkdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { REGRESSION_CASES, MODULE_CASES } from './parity-cases.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(__dirname, '..')

// Cases the Rust interpreter is known not to run yet. Keep the reason with
// the name: a bare list of names decays into a list of things nobody
// remembers being broken.
const UNSUPPORTED = new Set([
  // Generators. Both other implementations suspend one by delegating to a
  // host coroutine -- Python's `yield from`, JavaScript's `yield*` -- and
  // stable Rust has no equivalent to delegate to.
  'example: coroutines.mrt',
  'example: generators.mrt',
  'example: lazy.mrt',
  'regression: a generator function is lazy and yields in order',
  'regression: an endless generator is consumed only as far as asked',
  'regression: generators chain into streaming pipelines',
  'regression: yield inside if, for, for-in, try and match',
  'regression: return ends a generator early',
  'regression: generators are single use',
  'regression: a generator keeps its own scope across suspensions',
  'regression: errors and throws propagate out of a generator with a frame',
  'regression: closures capture a generator per call, independently',
  'regression: a generator method on a struct sees this',
  'regression: next/send drive a generator by hand and report done',
  'regression: the value sent on the first step is discarded',
  'regression: a yield with no sender sees null, and assigns through any target',
  'regression: next/send reject non-generators and tolerate exhaustion',
  'regression: yield* delegates to generators, arrays, strings and objects',
  'regression: yield* forwards sent values into the delegate',
  'regression: a generator resumes where an earlier consumer stopped',
  'regression: an exhausted generator is an error to iterate but not to step',
  'regression: resuming a generator from inside itself is an error',
  'regression: lazy map and filter over an endless generator',
  'regression: lazy map is only as lazy as it is asked to be',
  'regression: take pulls exactly as many items as asked',
  'regression: a struct with iter() is iterable everywhere an iterable is',
  'regression: the higher-order builtins accept anything iterable',
  'regression: abandoning many generators does not corrupt the interpreter',
  'regression: an abandoned generator runs no finally, a finished one does',
  'regression: abandoning a generator mid-loop leaves the caller intact',

  // Modules. No loader yet; the interpreter runs a single file.
  'example: json.mrt',
  'example: modules.mrt',
  'modules: named imports, aliasing, and nested module paths',
  'modules: a module is evaluated once no matter how many importers',
  'modules: module top-level code runs before the entry main()',
  'modules: private names are not importable',
  'modules: importing a name a module does not export',
  'modules: a missing module reports the specifier',
  'modules: a bare specifier is rejected',
  'modules: circular imports are detected and named',
  'modules: imported closures keep sharing their module state',
  'modules: namespace imports bind one object of every export',
  'modules: re-exports forward another module without binding locally',
  'modules: re-exporting a name the source module lacks',
  'modules: destructuring an imported object across module boundaries',
  'modules: a struct declared in one module is usable from another',
  'modules: modules combine with defaults, rest, errors and interpolation',
])

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
  return run('python3', ['-m', 'src', file])
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
