#!/usr/bin/env node
// Bundles the browser Playground's TypeScript interpreter (src/lib/mrtInterpreter.ts)
// with esbuild and checks that it produces byte-identical output to the Python
// reference interpreter (src/interpreter.py) for:
//   1. every bundled examples/*.mrt program, and
//   2. a set of inline regression snippets targeting bugs found in the two
//      independent implementations diverging (see the case list below).
//
// This exists because the two interpreters are hand-written to mirror each
// other rather than sharing code, and "the Python tests pass" says nothing
// about whether the Playground someone actually opens in a browser behaves
// the same way. Run with `npm run check:parity`.

import { execFileSync } from 'node:child_process'
import { readFileSync, readdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(__dirname, '..')

// -- Regression snippets -----------------------------------------------------
// Each of these caught a real divergence during development; keep them here
// so a future change can't silently reintroduce the same bug.
const REGRESSION_CASES = [
  {
    name: 'compound assignment on variable and array element',
    src: 'func main() { var x = 10; x += 5; print(x); var a = [1,2]; a[0] *= 10; print(a); }',
  },
  {
    name: 'object literal, dot access, dot compound-assign',
    src: 'func main() { var d = {"a": 1}; print(d); print(d.a); d.a += 10; print(d.a); }',
  },
  {
    name: 'keys/values/has/get on objects and arrays',
    src: 'func main() { var d = {"a":1,"b":2}; print(keys(d)); print(values(d)); print(has(d,"a")); print(get(d,"z","x")); print(has([1,2],2)); }',
  },
  {
    name: 'type() across every value kind',
    src: 'func main() { print(type(5), type("s"), type(true), type([1]), type({"a":1})); }',
  },
  {
    name: 'math builtins',
    src: 'func main() { print(abs(-5), min(3,1,2), max([3,9,1]), round(3.14159,2), floor(3.9), ceil(3.1), sqrt(16), pow(2,10)); }',
  },
  {
    name: 'toNumber/toString round-trip',
    src: 'func main() { print(toNumber("42") + 1); print(toString(42) + "!"); }',
  },
  {
    name: 'structural equality for arrays/objects, bool never equals number',
    src: 'func main() { print([1,[2,3]] == [1,[2,3]]); print(true == 1); print([true] == [1]); }',
  },
  {
    // Regression: a plain JS object literal used for the lexer's keyword
    // table made `KEYWORDS['toString']` resolve to the *inherited*
    // Object.prototype.toString method instead of undefined, corrupting
    // that token's type and breaking any MRT program that so much as
    // named a function `toString` (or `valueOf`, `constructor`, etc).
    name: 'identifiers that collide with Object.prototype members',
    src: 'func toString() { return 1; } func hasOwnProperty() { return 2; } func valueOf() { return 3; } func constructor() { return 4; } func main() { print(toString(), hasOwnProperty(), valueOf(), constructor()); }',
  },
  {
    name: 'indexOf/has use structural equality, not reference equality',
    src: 'func main() { var arr = [[1,2],[3,4]]; print(indexOf(arr,[3,4])); print(has(arr,[1,2])); }',
  },
]

function runPython(src) {
  const out = execFileSync('python3', ['-m', 'src'], {
    input: src,
    cwd: repoRoot,
    encoding: 'utf8',
  })
  return out.trimEnd()
}

// python3 -m src needs a *file*, not stdin, so write the snippet to a temp file.
function runPythonFile(filePath) {
  return execFileSync('python3', ['-m', 'src', filePath], { cwd: repoRoot, encoding: 'utf8' }).trimEnd()
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

  function check(name, src) {
    checked++
    const pyOut = runPythonFile(writeTemp(bundleDir, name, src))
    const { output, errors } = runMRT(src)
    const tsOut = (errors.length ? errors : output).join('\n')
    if (pyOut !== tsOut) {
      failures++
      console.error(`\n✗ MISMATCH: ${name}`)
      console.error('  python:', JSON.stringify(pyOut))
      console.error('  ts:    ', JSON.stringify(tsOut))
    } else {
      console.log(`✓ ${name}`)
    }
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
    const src = readFileSync(path.join(examplesDir, file), 'utf8')
    check(`example: ${file}`, src)
  }

  for (const { name, src } of REGRESSION_CASES) {
    check(`regression: ${name}`, src)
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
