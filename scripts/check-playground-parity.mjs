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

  // --- First-class functions -----------------------------------------------
  {
    name: 'anonymous functions as values, passed and returned',
    src: 'func main() { var d = func(x) { return x * 2; }; print(d(21)); var apply = func(f, v) { return f(v); }; print(apply(d, 5)); print(type(d)); }',
  },
  {
    name: 'closures capture their defining scope, not the call site',
    src: 'func main() { var mk = func() { var n = 0; return func() { n += 1; return n; }; }; var c = mk(); c(); c(); print(c()); var c2 = mk(); print(c2()); }',
  },
  {
    name: 'map/filter/reduce/find/some/every',
    src: 'func main() { var xs = [1,2,3,4,5]; print(map(xs, func(x){ return x*x; })); print(filter(xs, func(x){ return x % 2 == 1; })); print(reduce(xs, func(a,b){ return a+b; })); print(reduce(xs, func(a,b){ return a+b; }, 100)); print(find(xs, func(x){ return x > 3; })); print(some(xs, func(x){ return x > 4; }), every(xs, func(x){ return x > 0; })); }',
  },
  {
    name: 'sort: default order, comparator, stability, non-mutation',
    src: 'func main() { var n = [3,1,2]; print(sort(n)); print(n); print(sort(["pear","apple","fig"])); print(sort(n, func(a,b){ return b - a; })); var recs = [[1,"b"],[0,"a"],[1,"a"]]; print(sort(recs, func(x,y){ return x[0] - y[0]; })); }',
  },
  {
    name: 'a named function is a value too, and recursion still resolves',
    src: 'func fact(n) { if (n <= 1) { return 1; } return n * fact(n - 1); } func main() { var f = fact; print(f(5)); print(map([1,2,3,4], fact)); }',
  },

  // --- try / catch / throw -------------------------------------------------
  {
    name: 'throw and catch a plain value',
    src: 'func main() { try { throw "boom"; } catch (e) { print("caught", e); } print("after"); }',
  },
  {
    name: 'finally runs on normal exit, on catch, and on return',
    src: 'func helper() { try { return "r"; } finally { print("f2"); } } func main() { try { print("body"); } finally { print("f1"); } try { throw 1; } catch (e) { print("c"); } finally { print("f3"); } print(helper()); }',
  },
  {
    name: 'built-in runtime errors are catchable with message and line',
    src: 'func main() { try { var a = [1]; print(a[99]); } catch (e) { print(e.message); print(type(e), keys(e)); } try { print(1/0); } catch (e) { print(e.message); } }',
  },
  {
    name: 'nested try, rethrow from a catch block',
    src: 'func main() { try { try { throw {"code": 7}; } catch (e) { print("inner", e.code); throw "again"; } } catch (e2) { print("outer", e2); } }',
  },
  {
    name: 'uncaught throw halts the program and reports the value',
    src: 'func main() { print("before"); throw {"code": 42}; print("never"); }',
  },
  {
    name: 'try/finally with no catch propagates the error',
    src: 'func main() { try { try { throw "x"; } finally { print("cleanup"); } } catch (e) { print("got", e); } }',
  },

  // --- Syntax ergonomics ---------------------------------------------------
  {
    name: 'null literal, its type and equality',
    src: 'func main() { var n = null; print(n, type(n), n == null, n != 5); var d = {"a": null}; print(d, has(d, "a")); if (n) { print("truthy"); } else { print("falsy"); } }',
  },
  {
    name: 'string interpolation, including nesting and escapes',
    src: 'func main() { var name = "Ada"; var xs = [1,2]; print("Hi ${name}, ${1 + 2}, ${xs}!"); print("nested ${ get({"a": 5}, "a") }"); print("escaped \\${name}"); print("${ func(x){ return x+1; }(41) }"); }',
  },
  {
    name: 'bareword object keys are string keys',
    src: 'func main() { var p = {name: "Ada", age: 36}; print(p); print(p.name, p["age"]); var name = "shadow"; print({(name): 1}); print({name: 1}); }',
  },
  {
    name: 'for-in over arrays, strings, and object keys',
    src: 'func main() { for (x in [10,20]) { print("e", x); } for (var c in "hi") { print("c", c); } for (k in {a:1,b:2}) { print("k", k); } for (x in [1,2,3]) { if (x == 2) { continue; } if (x == 3) { break; } print("f", x); } }',
  },
  {
    name: 'for-in binds a fresh variable per iteration, so closures differ',
    src: 'func main() { var fns = []; for (x in [1,2,3]) { push(fns, func() { return x; }); } print(map(fns, func(f){ return f(); })); }',
  },

  // --- Standard library ----------------------------------------------------
  {
    name: 'sequence builtins',
    src: 'func main() { print(reverse([1,2,3]), reverse("abc")); print(unique([1,1,2,[3],[3]])); print(flatten([[1],[2,[3]]]), flatten([[1],[2,[3]]], 2)); print(zip([1,2,3],["a","b"])); print(enumerate(["x","y"])); print(count([1,1,2],1), sum([1,2,3])); print(range(3), range(2,5), range(0,10,3), range(3,0,-1)); }',
  },
  {
    name: 'string builtins',
    src: 'func main() { print(repeat("ab",3)); print(padStart("7",3,"0"), padEnd("7",3,".")); print(padStart("toolong",2)); print(padStart("x",5,"ab")); }',
  },
  {
    name: 'seeded random is identical in both interpreters',
    src: 'func main() { var r = random(42); var out = []; for (i in range(5)) { push(out, floor(r() * 1000)); } print(out); var r2 = random(42); print(floor(r2() * 1000)); var r3 = random(7); print(floor(r3() * 1000)); }',
  },
  {
    // Printing a built-in used to render the host language's own function
    // repr -- in Python complete with a memory address, so the output was
    // not even stable between runs, let alone equal to JavaScript's.
    name: 'function values stringify identically, built-ins included',
    src: 'func named() { return 1; } func main() { print(named); print(func(x) { return x; }); print(len); print(random(1)); print(toString(map)); print([named, len]); }',
  },
  {
    name: 'pipeline combining closures, for-in, interpolation and stdlib',
    src: 'func main() { var people = [{name: "Ada", age: 36}, {name: "Bob", age: 17}, {name: "Cy", age: 44}]; var adults = filter(people, func(p) { return p.age >= 18; }); var names = sort(map(adults, func(p) { return p.name; })); for (n in names) { print("adult: ${n}"); } print("total age ${ reduce(map(people, func(p){ return p.age; }), func(a,b){ return a+b; }) }"); }',
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
