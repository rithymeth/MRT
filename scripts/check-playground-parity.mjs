#!/usr/bin/env node
// Bundles the browser Playground's TypeScript interpreter (src/lib/mrtInterpreter.ts)
// with esbuild and checks that it produces byte-identical output to the Python
// reference interpreter (mrt/interpreter.py) for:
//   1. every bundled examples/*.mrt program, and
//   2. a set of inline regression snippets targeting bugs found in the two
//      independent implementations diverging (see the case list below).
//
// This exists because the two interpreters are hand-written to mirror each
// other rather than sharing code, and "the Python tests pass" says nothing
// about whether the Playground someone actually opens in a browser behaves
// the same way. Run with `npm run check:parity`.

import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync, mkdtempSync, writeFileSync, mkdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { REGRESSION_CASES, MODULE_CASES } from './parity-cases.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(__dirname, '..')

function runPython(src) {
  const out = execFileSync('python3', ['-m', 'mrt'], {
    input: src,
    cwd: repoRoot,
    encoding: 'utf8',
  })
  return out.trimEnd()
}

// python3 -m mrt needs a *file*, not stdin, so write the snippet to a temp file.
//
// A syntax error makes the CLI exit 65 and write to stderr, which execFileSync
// turns into a thrown error. That is still a result worth comparing -- the two
// implementations each have their own parser -- so the failure is caught and
// its stderr returned, letting syntax-error cases be checked for parity too.
function runPythonFile(filePath) {
  try {
    return execFileSync('python3', ['-m', 'mrt', filePath], {
      cwd: repoRoot,
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'pipe'],
    }).trimEnd()
  } catch (err) {
    if (err.stdout === undefined && err.stderr === undefined) throw err
    return `${err.stdout ?? ''}${err.stderr ?? ''}`.trimEnd()
  }
}

async function main() {
  const esbuild = await import('esbuild')
  const bundleDir = mkdtempSync(path.join(tmpdir(), 'mrt-parity-'))
  const bundlePath = path.join(bundleDir, 'mrtInterpreter.mjs')

  await esbuild.build({
    entryPoints: [path.join(repoRoot, 'src/lib/mrtInterpreter.ts')],
    bundle: true,
    format: 'esm',
    outfile: bundlePath,
    logLevel: 'silent',
  })

  const { runMRT } = await import(bundlePath)

  let failures = 0
  let checked = 0

  function report(name, pyOut, tsOut) {
    checked++
    if (pyOut !== tsOut) {
      failures++
      console.error(`\n✗ MISMATCH: ${name}`)
      console.error('  python:', JSON.stringify(pyOut))
      console.error('  ts:    ', JSON.stringify(tsOut))
    } else {
      console.log(`✓ ${name}`)
    }
  }

  // Resolves imports off the real filesystem, mirroring what the Python
  // interpreter does. The browser Playground plugs its own virtual-file
  // resolver into this same seam.
  const diskResolver = (specifier, fromPath) => {
    const resolved = path.resolve(path.dirname(fromPath), specifier)
    try {
      return { path: resolved, source: readFileSync(resolved, 'utf8') }
    } catch {
      return null
    }
  }

  function check(name, src, filePath = null) {
    const entry = filePath ?? writeTemp(bundleDir, name, src)
    const pyOut = runPythonFile(entry)
    const { output, errors } = runMRT(src, { path: entry, resolveModule: diskResolver })
    report(name, pyOut, (errors.length ? errors : output).join('\n'))
  }

  // A multi-file case: every entry in `files` is written into its own temp
  // directory and `main.mrt` is the entry point. Python resolves imports off
  // the real filesystem; the TypeScript interpreter is handed a resolver
  // that does the same, which is exactly the seam the browser Playground
  // fills with virtual files instead.
  function checkFiles(name, files) {
    const safe = name.replace(/[^a-z0-9]+/gi, '_').slice(0, 60)
    const root = path.join(bundleDir, `mod_${safe}`)
    for (const [relative, contents] of Object.entries(files)) {
      const full = path.join(root, relative)
      mkdirSync(path.dirname(full), { recursive: true })
      writeFileSync(full, contents)
    }

    const entry = path.join(root, 'main.mrt')
    const pyOut = runPythonFile(entry)

    const { output, errors } = runMRT(files['main.mrt'], { path: entry, resolveModule: diskResolver })
    report(name, pyOut, (errors.length ? errors : output).join('\n'))
  }

  function writeTemp(dir, name, src) {
    const safe = name.replace(/[^a-z0-9]+/gi, '_').slice(0, 60)
    const p = path.join(dir, `${safe}.mrt`)
    writeFileSync(p, src)
    return p
  }

  const examplesDir = path.join(repoRoot, 'examples')
  for (const file of readdirSync(examplesDir).sort()) {
    if (!file.endsWith('.mrt')) continue
    const full = path.join(examplesDir, file)
    // Pass the real path so an example that imports a sibling module
    // resolves the same way in both interpreters.
    check(`example: ${file}`, readFileSync(full, 'utf8'), full)
  }

  for (const { name, src } of REGRESSION_CASES) {
    check(`regression: ${name}`, src)
  }

  for (const { name, files } of MODULE_CASES) {
    checkFiles(`modules: ${name}`, files)
  }

  console.log(`\n${checked - failures}/${checked} checks matched.`)
  if (failures > 0) {
    console.error(`${failures} parity mismatch(es) between the Python interpreter and the Playground's TypeScript interpreter.`)
    process.exit(1)
  }
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
