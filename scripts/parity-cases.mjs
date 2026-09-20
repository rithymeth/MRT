// The shared conformance corpus.
//
// Every case here is a program with one right answer, and that answer is
// whatever `mrt/interpreter.py` prints. The corpus is imported by each
// harness that checks another implementation against it -- the Playground's
// TypeScript interpreter and the Rust one -- so a case added for one is
// automatically run by the others. That is the point: three hand-written
// implementations of the same language stay honest only if they are all
// answering the same questions.

import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = path.dirname(fileURLToPath(import.meta.url))

// -- Regression snippets -----------------------------------------------------
// Each of these caught a real divergence during development; keep them here
// so a future change can't silently reintroduce the same bug.
export const REGRESSION_CASES = [
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
    // round() used to differ: Python rounded half-to-even on the exact
    // binary value while the Playground rounded half-up on the scaled one,
    // so anything landing near a .5 boundary disagreed. Both now run the
    // same explicit algorithm.
    //
    // Keep every literal here in a form MRT can actually lex. This case
    // once contained `1e10`, and MRT has no exponent notation -- it lexes
    // as `1` followed by the identifier `e10`, which made the whole
    // snippet a syntax error. Both interpreters then "agreed" on that
    // error and the case silently tested nothing at all.
    name: 'round() agrees on half-way values, negatives and digit counts',
    src: 'func main() { print(round(3.14159 * 2500, 2)); print(round(2.5), round(-2.5), round(3.5), round(-3.5), round(0.5), round(-0.5)); print(round(1.005, 2), round(-1.005, 2), round(2.675, 2)); print(round(1234.5678, -2), round(1234.5678, 2), round(1234.5678)); print(round(0), round(-0.0), round(10000000000, 2)); }',
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
  // --- Richer errors: kind, stack, guarded catch ---------------------------
  {
    name: 'error kind classification across the language',
    src: 'func main() { var probes = [func(){ var a=[]; return a[5]; }, func(){ return 1/0; }, func(){ return undefinedThing; }, func(){ return -"x"; }, func(){ return len(); }, func(){ return sqrt(-1); }, func(){ var o={a:1}; return o.missing; }]; for (p in probes) { try { p(); } catch (e) { print(e.kind); } } }',
  },
  {
    name: 'error stack trace is innermost-first across frames',
    src: 'func c() { var a = [1]; return a[99]; } func b() { return c(); } func a() { return b(); } func main() { try { a(); } catch (e) { print(e.stack); print(len(e.stack)); } }',
  },
  {
    name: 'catch guards select by kind, unguarded clause is the fallback',
    src: 'func attempt(n) { try { if (n == 0) { var a=[]; print(a[9]); } if (n == 1) { print(1/0); } if (n == 2) { print(nope); } if (n == 3) { throw {code: "USER"}; } } catch (e) if (get(e, "kind", "") == "IndexError") { print("index"); } catch (e) if (get(e, "kind", "") == "ArithmeticError") { print("math"); } catch (e) if (get(e, "kind", "") == "NameError") { print("name"); } catch (e) { print("fallback", get(e, "code", "?")); } } func main() { for (n in range(4)) { attempt(n); } }',
  },
  {
    // A guard is ordinary code: if it raises (here, reading `.kind` off a
    // thrown value that has no such key), that error replaces the original
    // rather than being swallowed. Hence the `get(e, "kind", "")` idiom in
    // the case above.
    name: 'an error raised inside a catch guard propagates',
    src: 'func main() { try { try { throw "a plain string"; } catch (e) if (e.kind == "IndexError") { print("never"); } } catch (outer) { print(outer.kind, "|", outer.message); } }',
  },
  {
    name: 'an unmatched guard lets the error keep propagating, finally still runs',
    src: 'func main() { try { try { throw {code: "X"}; } catch (e) if (e.code == "Y") { print("never"); } finally { print("inner finally"); } } catch (e) { print("outer", e.code); } }',
  },
  {
    name: 'caught error object shape and key order',
    src: 'func main() { try { var a = [1]; print(a[7]); } catch (e) { print(keys(e)); print(type(e.line), type(e.kind), type(e.stack)); } }',
  },
  {
    // Indexing raises from a helper several frames below the expression, and
    // for a long time nothing put the line back: every IndexError, KeyError
    // and bad-key TypeError arrived with `line` null and printed without the
    // `[line N]` suffix. Two implementations agreed on it, so parity said
    // nothing; the third one reported the line and gave it away. Pin the
    // values, not just the types -- `type(e.line)` was already being checked
    // above and passed throughout.
    name: 'indexing errors carry the line they were written on',
    src: [
      'func probe(f) { try { f(); } catch (e) { print(e.kind, e.line); } }',
      'func main() {',
      '  probe(func() { var a = [1]; return a[7]; });',
      '  probe(func() { var a = [1]; a[7] = 0; });',
      '  probe(func() { var o = {a: 1}; return o.zz; });',
      '  probe(func() { var o = {a: 1}; return o["zz"]; });',
      '  probe(func() { var a = [1]; return a["x"]; });',
      '  probe(func() { return 5[0]; });',
      '  probe(func() { var s = "hi"; s[0] = "H"; });',
      '  probe(func() { var o = {}; o[[1]] = 1; });',
      '  probe(func() { return {([1]): 1}; });',
      '}',
    ].join('\n'),
  },
  {
    name: 'a rethrown error from a guard-matched clause reaches the outer try',
    src: 'func inner() { var a = []; return a[0]; } func main() { try { try { inner(); } catch (e) if (e.kind == "IndexError") { throw "converted: ${e.kind}"; } } catch (e2) { print(e2); } }',
  },
  {
    // get() is documented as the accessor that never raises, but it used to
    // throw on anything that wasn't an array or object -- which made the
    // natural catch guard `get(e, "kind", "")` blow up on a thrown string.
    name: 'get() is total: any non-container yields the default',
    src: 'func main() { print(get("abc", 1, "?"), get("abc", "k", "?"), get(5, "k", "?"), get(null, "k", "?"), get(true, "k", "?"), get(len, "k", "?")); print(get([1,2], 0, "?"), get([1,2], 9, "?"), get({a:1}, "a", "?"), get({a:1}, "z", "?")); print(get(5, "k")); }',
  },
  // --- Default, rest and spread parameters ---------------------------------
  {
    name: 'default parameter values, filled left to right',
    src: 'func greet(name, greeting = "Hello", punct = "!") { return "${greeting}, ${name}${punct}"; } func main() { print(greet("Ada")); print(greet("Ada", "Hi")); print(greet("Ada", "Hi", "?")); }',
  },
  {
    name: 'a default may refer to an earlier parameter, evaluated per call',
    src: 'func chained(a, b = a * 2, c = a + b) { return [a, b, c]; } func counterDefault(n, tag = "n=${n}") { return tag; } func main() { print(chained(1)); print(chained(1, 10)); print(chained(2, 3, 4)); print(counterDefault(7)); }',
  },
  {
    name: 'rest parameters collect the remainder, empty when there is none',
    src: 'func total(label, ...nums) { return "${label}: ${sum(nums)} (${len(nums)})"; } func onlyRest(...xs) { return xs; } func main() { print(total("empty")); print(total("three", 1, 2, 3)); print(onlyRest()); print(onlyRest(1, "a", [2])); }',
  },
  {
    name: 'spread in calls, array literals and print',
    src: 'func total(label, ...nums) { return sum(nums); } func main() { var a = [1,2]; var b = [3,4]; print(total("x", ...a)); print(total("x", 10, ...a, 20)); print([...a, ...b]); print([0, ...a, 99]); print([...[]]); print(...a); print("v:", ...b, "end"); }',
  },
  {
    name: 'defaults and rest on anonymous functions and closures',
    src: 'func main() { var f = func(x, y = 10, ...rest) { return [x, y, rest]; }; print(f(1)); print(f(1, 2, 3, 4)); var make = func(prefix = ">") { return func(...parts) { return prefix + join(parts, ","); }; }; var g = make(); print(g("a", "b")); var h = make("* "); print(h("c")); }',
  },
  {
    name: 'arity errors describe the accepted range',
    src: 'func exact(a, b) { return 0; } func defaulted(a, b = 1, c = 2) { return 0; } func variadic(a, ...r) { return 0; } func main() { var probes = [func(){ return exact(1); }, func(){ return exact(1,2,3); }, func(){ return defaulted(); }, func(){ return defaulted(1,2,3,4); }, func(){ return variadic(); }]; for (p in probes) { try { p(); } catch (e) { print(e.kind, "|", e.message); } } }',
  },
  {
    name: 'spreading a non-array is a type error',
    src: 'func f(...xs) { return len(xs); } func main() { var probes = [func(){ return f(...5); }, func(){ return f(..."ab"); }, func(){ return f(...{a:1}); }, func(){ return f(...null); }]; for (p in probes) { try { p(); } catch (e) { print(e.kind, "|", e.message); } } }',
  },
  // --- Destructuring -------------------------------------------------------
  {
    name: 'array destructuring in var, with rest and defaults',
    src: 'func main() { var [x, y] = [1, 2]; print(x, y); var [head, ...tail] = [1,2,3,4]; print(head, tail); var [p, q = 99] = [5]; print(p, q); var [only, ...none] = [7]; print(only, none); }',
  },
  {
    name: 'object destructuring in var, with renaming, defaults and rest',
    src: 'func main() { var {name, age} = {name: "Ada", age: 36}; print(name, age); var {name: who, missing = "n/a"} = {name: "Bob"}; print(who, missing); var {a, ...others} = {a: 1, b: 2, c: 3}; print(a, others); }',
  },
  {
    name: 'nested patterns mixing arrays and objects',
    src: 'func main() { var [{id}, [inner]] = [{id: 7}, [8]]; print(id, inner); var {items: [first, second], meta: {tag}} = {items: [1,2], meta: {tag: "t"}}; print(first, second, tag); }',
  },
  {
    name: 'destructuring parameters, including defaults on the whole pattern',
    src: 'func swap([a, b]) { return [b, a]; } func describe({name, age = 0, ...rest}) { return "${name}/${age}/${keys(rest)}"; } func opt({x} = {x: 5}) { return x; } func main() { print(swap([1,2])); print(describe({name: "Cy", extra: true})); print(opt()); print(opt({x: 9})); }',
  },
  {
    name: 'destructuring in for-in and catch',
    src: 'func main() { for ([k, v] in [["a",1],["b",2]]) { print(k, "=", v); } for ({name: n} in [{name:"x"},{name:"y"}]) { print(n); } try { var a = []; print(a[3]); } catch ({kind, message}) { print(kind, "|", message); } }',
  },
  {
    name: 'destructuring failures are strict and classified',
    src: 'func main() { var probes = [func(){ var [m, n] = [1]; return m; }, func(){ var {zz} = {}; return zz; }, func(){ var [w] = 5; return w; }, func(){ var {w} = [1]; return w; }, func(){ var [[q]] = [[]]; return q; }]; for (p in probes) { try { p(); } catch (e) { print(e.kind, "|", e.message); } } }',
  },
  {
    name: 'destructured for-in still binds fresh per iteration',
    src: 'func main() { var fns = []; for ([a, b] in [[1,2],[3,4]]) { push(fns, func() { return a + b; }); } print(map(fns, func(f){ return f(); })); }',
  },

  // --- Structs -------------------------------------------------------------
  {
    name: 'struct construction, fields, stringify and type()',
    src: 'struct Point { x, y; } func main() { var p = Point(3, 4); print(p); print(p.x, p.y); print(type(p), type(Point)); print(toString(p)); }',
  },
  {
    name: 'methods see `this`, and stay bound when pulled off the instance',
    src: 'struct Point { x, y; func mag() { return sqrt(this.x*this.x + this.y*this.y); } func scaled(k) { return Point(this.x*k, this.y*k); } func label(prefix = "P") { return "${prefix}(${this.x},${this.y})"; } } func main() { var p = Point(3,4); print(p.mag()); print(p.scaled(2)); print(p.label(), p.label("Q")); var m = p.mag; print(m()); }',
  },
  {
    name: 'field defaults, including ones referring to earlier fields',
    src: 'struct Config { host, port = 8080, url = "http://${host}:${port}"; } func main() { print(Config("a")); print(Config("a", 99)); print(Config("a", 1, "custom")); }',
  },
  {
    name: 'field assignment is restricted to declared fields',
    src: 'struct P { x; } func main() { var p = P(1); p.x = 5; print(p.x); try { p.y = 1; } catch (e) { print(e.kind, "|", e.message); } try { print(p.z); } catch (e) { print(e.kind, "|", e.message); } }',
  },
  {
    name: 'struct equality is structural and per-struct',
    src: 'struct A { v; } struct B { v; } func main() { print(A(1) == A(1), A(1) == A(2), A(1) == B(1)); print(A([1,2]) == A([1,2])); print(A(1) == {v: 1}); print(indexOf([A(1), A(2)], A(2))); }',
  },
  {
    name: 'structs answer keys/values/has/get/len and object destructuring',
    src: 'struct Point { x, y; func mag() { return 0; } } func main() { var p = Point(3,4); print(keys(p), values(p), len(p)); print(has(p,"x"), has(p,"mag"), get(p,"x",0), get(p,"zz","d")); var {x, y} = p; print(x, y); var {x: a, ...rest} = p; print(a, rest); }',
  },
  {
    name: 'struct constructor arity errors',
    src: 'struct P { x, y; } struct Q { a, b = 2; } func main() { var probes = [func(){ return P(1); }, func(){ return P(1,2,3); }, func(){ return Q(); }]; for (f in probes) { try { f(); } catch (e) { print(e.kind, "|", e.message); } } }',
  },
  {
    name: 'structs are hoisted, nest, and work with the collection pipeline',
    src: 'func main() { var people = [Person("Ada", 36), Person("Bob", 17), Person("Cy", 44)]; var adults = filter(people, func(p) { return p.isAdult(); }); print(map(adults, func(p) { return p.name; })); print(sort(map(people, func(p){ return p.age; }))); var nested = Wrapper(Person("Zed", 1)); print(nested.inner.name); } struct Person { name, age; func isAdult() { return this.age >= 18; } } struct Wrapper { inner; }',
  },
  // --- Pattern matching ----------------------------------------------------
  {
    name: 'literal patterns, including negatives, booleans and null',
    src: 'func classify(v) { match (v) { case 0: return "zero"; case -1: return "minus one"; case "hi": return "greeting"; case true: return "yes"; case null: return "nothing"; case n: return "other ${toString(n)}"; } } func main() { for (v in [0, -1, "hi", true, null, 7, "x"]) { print(classify(v)); } }',
  },
  {
    name: 'array patterns match length exactly unless a rest is given',
    src: 'func f(v) { match (v) { case []: return "empty"; case [a]: return "one ${a}"; case [a, b]: return "two ${a},${b}"; case [h, ...t]: return "many ${h}+${len(t)}"; case n: return "not an array"; } } func main() { print(f([])); print(f([1])); print(f([1,2])); print(f([1,2,3,4])); print(f("s")); }',
  },
  {
    name: 'object patterns are partial and can nest',
    src: 'func f(v) { match (v) { case {kind: "circle", radius: r}: return "circle ${r}"; case {kind: k, meta: {tag: t}}: return "${k}/${t}"; case {kind: k}: return "tagged ${k}"; case other: return "none"; } } func main() { print(f({kind: "circle", radius: 5, extra: 1})); print(f({kind: "sq", meta: {tag: "t"}})); print(f({kind: "sq"})); print(f({nope: 1})); print(f(5)); }',
  },
  {
    name: 'struct patterns match the exact struct and bind fields in order',
    src: 'struct Circle { radius; } struct Rect { w, h; } func area(s) { match (s) { case Circle(r): return round(3.14159 * r * r, 2); case Rect(w, h): return w * h; default: return -1; } } func main() { print(area(Circle(2)), area(Rect(3,4)), area("x"), area(5)); }',
  },
  {
    name: 'case guards, tried in order, first match wins',
    src: 'func size(n) { match (n) { case x if (x < 0): return "negative"; case 0: return "zero"; case x if (x > 100): return "big"; case x: return "small"; } } func main() { for (n in [-5, 0, 500, 7]) { print(size(n)); } }',
  },
  {
    name: 'nested patterns combining arrays, objects and structs',
    src: 'struct P { x, y; } func f(v) { match (v) { case [P(0, 0), rest]: return "origin then ${toString(rest)}"; case [P(x, y), ...more]: return "point ${x},${y} +${len(more)}"; case {items: [first, second]}: return "items ${first}/${second}"; default: return "no"; } } func main() { print(f([P(0,0), "tail"])); print(f([P(1,2), 9, 9])); print(f({items: [1,2]})); print(f(3)); }',
  },
  {
    name: 'no matching case without a default is an error',
    src: 'func main() { print("before"); match (99) { case 1: print("one"); } print("never"); }',
  },
  {
    name: 'match bindings are scoped to their case',
    src: 'func main() { var n = "outer"; match ([1, 2]) { case [n, m]: print("inside", n, m); default: print("no"); } print("after", n); }',
  },
  {
    name: 'a struct pattern with the wrong field count is an error',
    src: 'struct P { x, y; } func main() { try { match (P(1,2)) { case P(a): print("no"); default: print("d"); } } catch (e) { print(e.kind, "|", e.message); } try { var notStruct = 5; match (1) { case notStruct(a): print("no"); } } catch (e) { print(e.kind, "|", e.message); } }',
  },

  // --- Generators ----------------------------------------------------------
  {
    name: 'a generator function is lazy and yields in order',
    src: 'func countdown(start) { var n = start; while (n > 0) { yield n; n -= 1; } yield "liftoff"; } func main() { print(type(countdown), type(countdown(1))); print(toString(countdown(1))); for (v in countdown(3)) { print(v); } }',
  },
  {
    name: 'an endless generator is consumed only as far as asked',
    src: 'func naturals() { var n = 0; while (true) { yield n; n += 1; } } func main() { print(take(naturals(), 5)); var out = []; for (n in naturals()) { if (n > 4) { break; } push(out, n); } print(out); }',
  },
  {
    name: 'generators chain into streaming pipelines',
    src: 'func naturals() { var n = 0; while (true) { yield n; n += 1; } } func squares(src) { for (x in src) { yield x * x; } } func takeWhile(src, pred) { for (x in src) { if (!pred(x)) { return; } yield x; } } func main() { print(toArray(takeWhile(squares(naturals()), func(v) { return v < 100; }))); print(take(squares(naturals()), 4)); }',
  },
  {
    name: 'yield inside if, for, for-in, try and match',
    src: 'func mixed() { for (var i = 0; i < 2; i += 1) { if (i == 0) { yield "if"; } else { yield "else"; } } for (c in "ab") { yield c; } try { yield "try"; throw "x"; } catch (e) { yield "catch ${e}"; } finally { yield "finally"; } match (2) { case 2: yield "match"; default: yield "no"; } } func main() { print(toArray(mixed())); }',
  },
  {
    name: 'return ends a generator early',
    src: 'func upTo(limit) { var n = 0; while (true) { if (n > limit) { return; } yield n; n += 1; } } func main() { print(toArray(upTo(3))); print(toArray(upTo(0))); }',
  },
  {
    name: 'generators are single use',
    src: 'func two() { yield 1; yield 2; } func main() { var g = two(); print(toArray(g)); try { print(toArray(g)); } catch (e) { print(e.kind, "|", e.message); } }',
  },
  {
    name: 'a generator keeps its own scope across suspensions',
    src: 'func counter() { var n = 0; while (n < 3) { n += 1; yield n; } } func main() { var g = counter(); var outside = 100; for (v in g) { outside += 1; print(v, outside); } }',
  },
  {
    name: 'errors and throws propagate out of a generator with a frame',
    src: 'func boom() { yield 1; var a = []; yield a[9]; } func thrower() { yield 1; throw "from generator"; } func main() { try { print(toArray(boom())); } catch (e) { print(e.kind, "|", e.message, "|", e.stack); } try { for (v in thrower()) { print(v); } } catch (e) { print("caught", e); } }',
  },
  {
    name: 'closures capture a generator per call, independently',
    src: 'func gen(tag) { var i = 0; while (i < 2) { yield "${tag}${i}"; i += 1; } } func main() { var a = gen("a"); var b = gen("b"); print(toArray(a), toArray(b)); }',
  },
  {
    name: 'take and toArray work on every iterable, and reject the rest',
    src: 'func main() { print(toArray([1,2]), toArray("ab"), toArray({x:1,y:2})); print(take([1,2,3], 2), take("hello", 3), take([1], 0)); var probes = [func(){ return toArray(5); }, func(){ return take(5, 1); }, func(){ return take([1], -1); }]; for (p in probes) { try { p(); } catch (e) { print(e.kind, "|", e.message); } } }',
  },
  {
    name: 'a generator method on a struct sees this',
    src: 'struct Bag { items; func each() { for (x in this.items) { yield x; } } } func main() { var b = Bag([1,2,3]); print(toArray(b.each())); print(take(b.each(), 2)); }',
  },
  {
    name: 'yield outside a function is a syntax error',
    src: 'yield 1;',
  },


  // --- Two-way generators, delegation and lazy pipelines -------------------
  {
    name: 'next/send drive a generator by hand and report done',
    src: 'func echo() { var got = yield "ready"; while (got != "stop") { got = yield "saw ${got}"; } yield "bye"; } func main() { var g = echo(); print(next(g)); print(send(g, "a")); print(send(g, "b")); print(send(g, "stop")); print(next(g), next(g)); }',
  },
  {
    name: 'the value sent on the first step is discarded',
    src: 'func g() { var first = yield 1; print("got", first); yield 2; } func main() { var it = g(); print(send(it, "ignored")); print(send(it, "kept")); }',
  },
  {
    name: 'a yield with no sender sees null, and assigns through any target',
    src: 'func g() { var a = yield 1; print("a", a); var o = {k: 0}; o.k = yield 2; print("k", o.k); var p = 0; p = yield 3; print("p", p); } func pair() { var a = 0; var b = 0; [a, b] = yield "give me two"; print("pair", a, b); } func main() { print(toArray(g())); var h = g(); print(send(h, 10), send(h, 20), send(h, 30), next(h)); var q = pair(); print(next(q)); print(send(q, [7, 8])); var r = pair(); try { toArray(r); } catch (e) { print(e.kind, "|", e.message); } }',
  },
  {
    name: 'next/send reject non-generators and tolerate exhaustion',
    src: 'func g() { yield 1; } func main() { var probes = [func(){ return next(5); }, func(){ return send([1], 2); }, func(){ return next(); }, func(){ return send(g()); }]; for (p in probes) { try { p(); } catch (e) { print(e.kind, "|", e.message); } } var it = g(); print(next(it), next(it), next(it)); }',
  },
  {
    name: 'yield* delegates to generators, arrays, strings and objects',
    src: 'func inner() { yield 1; yield 2; } func outer() { yield 0; yield* inner(); yield* [8, 9]; yield* "ab"; yield* {k: 1}; yield 3; } func main() { print(toArray(outer())); }',
  },
  {
    name: 'yield* forwards sent values into the delegate',
    src: 'func inner() { var a = yield "i1"; var b = yield "i2:${a}"; print("inner saw", a, b); } func outer() { yield "o"; yield* inner(); } func main() { var g = outer(); print(next(g)); print(send(g, "x")); print(send(g, "y")); print(send(g, "z")); }',
  },
  {
    name: 'yield* as an expression is a syntax error',
    src: 'func f() { var x = yield* [1]; } func main() { f(); }',
  },
  {
    name: 'yield in a nested expression is a syntax error',
    src: 'func f() { print(1 + yield 2); } func main() { f(); }',
  },
  {
    name: 'a generator resumes where an earlier consumer stopped',
    src: 'func naturals() { var n = 0; while (true) { yield n; n += 1; } } func main() { var g = naturals(); print(take(g, 3)); print(take(g, 2)); for (v in g) { if (v > 6) { break; } } print(next(g)); }',
  },
  {
    name: 'an exhausted generator is an error to iterate but not to step',
    src: 'func two() { yield 1; yield 2; } func main() { var g = two(); print(toArray(g)); try { toArray(g); } catch (e) { print(e.kind, "|", e.message); } print(next(g)); try { for (v in g) { print(v); } } catch (e) { print(e.kind, "|", e.message); } }',
  },
  {
    name: 'resuming a generator from inside itself is an error',
    src: 'var shared = null; func f() { yield 1; var again = next(shared); yield 2; } func main() { shared = f(); try { print(toArray(shared)); } catch (e) { print(e.kind, "|", e.message); } }',
  },
  {
    name: 'lazy map and filter over an endless generator',
    src: 'func naturals() { var n = 0; while (true) { yield n; n += 1; } } func main() { print(type(map(naturals(), func(n){ return n; }))); print(toString(map(naturals(), func(n){ return n; })), toString(filter(naturals(), func(n){ return true; }))); print(take(map(naturals(), func(n){ return n * n; }), 5)); print(take(filter(naturals(), func(n){ return n % 2 == 0; }), 4)); print(take(map(filter(naturals(), func(n){ return n % 3 == 0; }), func(n){ return "n${n}"; }), 3)); }',
  },
  {
    name: 'map and filter stay eager on arrays, and still reject other types',
    src: 'func main() { print(map([1,2], func(x){ return x + 1; }), filter([1,2,3], func(x){ return x > 1; })); var probes = [func(){ return map(5, func(x){ return x; }); }, func(){ return filter("ab", func(x){ return x; }); }, func(){ return map([1]); }, func(){ return filter([1]); }]; for (p in probes) { try { p(); } catch (e) { print(e.kind, "|", e.message); } } }',
  },
  {
    name: 'lazy map is only as lazy as it is asked to be',
    src: 'func three() { yield 1; yield 2; yield 3; } func main() { var calls = 0; var counted = map(three(), func(x) { calls += 1; return x * 2; }); print(calls); print(take(counted, 2)); print(calls); print(toArray(counted)); print(calls); }',
  },
  {
    name: 'take pulls exactly as many items as asked',
    src: 'func three() { yield 1; yield 2; yield 3; } func main() { var g = three(); print(take(g, 0)); print(next(g)); print(take(g, 10)); }',
  },

  // --- Destructuring assignment -------------------------------------------
  {
    name: 'array and object destructuring assignment, including swap',
    src: 'func main() { var a = 1; var b = 2; [a, b] = [b, a]; print(a, b); var x = 0; var y = 0; {x, y} = {x: 5, y: 6}; print(x, y); var head = 0; var rest = []; [head, ...rest] = [1,2,3]; print(head, rest); }',
  },
  {
    name: 'destructuring assignment nests, renames, defaults and rests',
    src: 'func main() { var a = 0; var b = 0; var c = 0; var others = {}; [a, [b, c]] = [1, [2, 3]]; print(a, b, c); {name: a, ...others} = {name: "Ada", x: 1, y: 2}; print(a, others); [b, c = 9] = [7]; print(b, c); }',
  },
  {
    name: 'destructuring assignment needs existing variables and the right shape',
    src: 'func main() { var a = 0; var probes = [func(){ [nope] = [1]; }, func(){ [a, a, a] = [1]; }, func(){ {k} = {}; }, func(){ [a] = 5; }]; for (p in probes) { try { p(); } catch (e) { print(e.kind, "|", e.message); } } }',
  },
  {
    name: 'a leading brace is still a block, and a leading bracket still an array',
    src: 'func main() { var x = 1; { x = 2; } print(x); { print("block"); } var arr = [1, 2]; [1, 2][0]; print(arr[0]); if (true) { x = 3; } print(x); }',
  },
  {
    name: 'destructuring assignment reaches outer scopes, not new ones',
    src: 'func main() { var a = 1; var b = 2; func inner() { [a, b] = [10, 20]; } inner(); print(a, b); for (i in [0]) { [a] = [99]; } print(a); }',
  },

  // --- match as an expression ---------------------------------------------
  {
    name: 'match expressions bind, guard and default',
    src: 'func classify(v) { return match (v) { case 0: "zero", case [a, b]: "pair ${a+b}", case {kind: "dog", name: n}: "dog ${n}", case n if (type(n) == "number" && n < 0): "neg ${n}", default: "other" }; } func main() { for (v in [0, -3, 5]) { print(classify(v)); } print(classify([1,2])); print(classify({kind: "dog", name: "Rex"})); print(classify("x")); }',
  },
  {
    name: 'match expressions nest and compose with calls and interpolation',
    src: 'struct Point { x, y } func main() { var p = Point(1, 2); print("at ${ match (p) { case Point(a, b): "${a},${b}", default: "?" } }"); var v = match (3) { case n if (n > 2): match (n) { case 3: "three", default: "big" }, default: "small" }; print(v); print(len(match (true) { case true: [1,2,3], default: [] })); }',
  },
  {
    name: 'an unmatched match expression is an error, and match is still a statement',
    src: 'func main() { try { print(match (99) { case 1: "one" }); } catch (e) { print(e.kind, "|", e.message); } match (2) { case 2: print("statement form"); default: print("no"); } var t = match (1) { case 1: "trailing comma ok", }; print(t); }',
  },

  // --- Iterator protocol ---------------------------------------------------
  {
    name: 'a struct with iter() is iterable everywhere an iterable is',
    src: 'struct Span { lo, hi; func iter() { var n = this.lo; while (n < this.hi) { yield n; n += 1; } } } func main() { var r = Span(1, 5); for (v in r) { print(v); } print(toArray(Span(0, 3)), take(Span(0, 9), 2)); print(map(Span(0, 3), func(n){ return n * 2; })); print(toArray(map(Span(0, 3), func(n){ return n; })) == [0, 1, 2]); }',
  },
  {
    name: 'iter() may return any iterable, and a struct without it iterates fields',
    src: 'struct Deck { cards; func iter() { return this.cards; } } struct Plain { a, b } func main() { print(toArray(Deck(["A","K"]))); for (k in Plain(1, 2)) { print(k); } }',
  },
  {
    name: 'an iter() that returns the struct itself is reported, not looped',
    src: 'struct Loop { n; func iter() { return this; } } struct Bad { n; func iter() { return this.n; } } func main() { try { for (x in Loop(1)) { print(x); } } catch (e) { print(e.kind, "|", e.message); } try { print(toArray(Bad(5))); } catch (e) { print(e.kind, "|", e.message); } }',
  },
  {
    name: 'the higher-order builtins accept anything iterable',
    src: 'struct Span { lo, hi; func iter() { var n = this.lo; while (n < this.hi) { yield n; n += 1; } } } func main() { var s = Span(0, 4); print(map(s, func(n){ return n * 2; })); print(filter(s, func(n){ return n % 2 == 0; })); print(reduce(Span(1, 5), func(a, b){ return a + b; })); print(find(s, func(n){ return n > 2; }), some(s, func(n){ return n > 9; }), every(s, func(n){ return n < 9; })); print(map("abc", func(c){ return toUpper(c); })); print(map({a: 1, b: 2}, func(k){ return k + "!"; })); var probes = [func(){ return map(5, func(x){ return x; }); }, func(){ return reduce(7, func(a,b){ return a; }); }, func(){ return sort(s); }, func(){ return reduce([], func(a,b){ return a; }); }]; for (p in probes) { try { p(); } catch (e) { print(e.kind, "|", e.message); } } }',
  },
  {
    name: 'from and as are contextual keywords, usable as ordinary names',
    src: 'struct Trip { from, to } func main() { var from = "here"; var as = "now"; print(from, as); var o = {from: 1, as: 2, to: 3}; print(o, o.from, o.as); var t = Trip("A", "B"); print(t, t.from); var {from: f} = o; print(f); for (from in [1, 2]) { print(from); } func as2(from, as) { return from + as; } print(as2(1, 2)); }',
  },

  // --- Abandoned generators ------------------------------------------------
  // Found by a benchmark, not a test. A suspended generator that nothing
  // refers to is garbage; CPython disposes of one by throwing GeneratorExit
  // at its `yield`, at a collection point in the middle of unrelated work.
  // The interpreter's `try`/`finally` cleanup then restored a stale scope
  // into the live interpreter. JavaScript never closes an abandoned
  // generator at all, so the two implementations disagreed -- and because
  // the failure depends on collection timing, nothing here caught it until a
  // benchmark ran the same loop a few hundred times.
  {
    name: 'abandoning many generators does not corrupt the interpreter',
    src: 'func naturals() { var n = 0; while (true) { yield n; n += 1; } } func main() { var total = 0; var rounds = 0; while (rounds < 300) { var g = naturals(); total += len(take(g, 3)); rounds += 1; } print(total); }',
  },
  {
    name: 'an abandoned generator runs no finally, a finished one does',
    src: 'func g() { try { yield 1; yield 2; } finally { print("cleanup"); } } func main() { var rounds = 0; while (rounds < 300) { take(g(), 1); rounds += 1; } print("dropped 300"); print(toArray(g())); }',
  },
  {
    name: 'abandoning a generator mid-loop leaves the caller intact',
    src: 'func naturals() { var n = 0; while (true) { yield n; n += 1; } } func main() { var sum = 0; var i = 0; while (i < 300) { var local = i * 2; for (v in naturals()) { if (v > 1) { break; } } sum += local; i += 1; } print(sum); }',
  },

  // --- Arithmetic and object-key semantics ---------------------------------
  // Both of these diverged silently until someone probed for them, because
  // nothing in this file used a negative modulo or mixed boolean and numeric
  // object keys.
  {
    name: 'modulo takes the sign of the dividend, as in C and JavaScript',
    src: 'func main() { print(7 % 3, -7 % 3, 7 % -3, -7 % -3); print(7.5 % 2, -7.5 % 2); print(0 % 5, -0 % 5); var probes = [func(){ return 1 % 0; }, func(){ return -1 % 0; }]; for (p in probes) { try { p(); } catch (e) { print(e.kind, "|", e.message); } } }',
  },
  {
    name: 'boolean and numeric object keys are distinct, as == says they are',
    src: 'func main() { var byNumber = {1: "one", 0: "zero"}; print(get(byNumber, true, "<missing>"), get(byNumber, false, "<missing>")); var byBool = {true: "yes"}; print(get(byBool, 1, "<missing>"), has(byBool, 1)); var mixed = {1: "number", true: "boolean"}; print(len(mixed), keys(mixed), values(mixed)); print(mixed[1], mixed[true]); print(mixed); var o = {}; o[true] = "t"; o[1] = "n"; o[false] = "f"; o[0] = "z"; print(len(o), keys(o)); for (k in o) { print(k, type(k), o[k]); } try { print(byNumber[true]); } catch (e) { print(e.kind, "|", e.message); } }',
  },

  {
    // Scoping cases that a compiler resolving names to frame slots is the
    // first thing to get wrong, and that produce a plausible-looking wrong
    // answer rather than a crash when it does.
    name: 'shadowing, slot reuse and capture live together correctly',
    src: [
      'func shadow() { var x = "outer"; { var x = "inner"; print(x); } print(x); return x; }',
      'func siblings() { { var a = 1; print(a); } { var b = 2; print(b); } { var c = 3; var d = 4; print(c, d); } }',
      'func mixed() { var kept = 0; var g = func() { kept = kept + 1; return kept; }; var plain = 100; print(g(), g(), plain); return kept; }',
      'func capturedParam(a, b) { var g = func() { return a; }; return g() + b; }',
      'func shadowParam(x) { { var x = x + 1; return x; } }',
      'func main() { print(shadow()); siblings(); print(mixed()); print(capturedParam(5, 6)); print(shadowParam(41)); }',
    ].join('\n'),
  },
  {
    name: 'a C-style loop variable is shared, a for-in one is per iteration',
    src: [
      'func cStyle() { var fs = []; for (var i = 0; i < 3; i = i + 1) { push(fs, func() { return i; }); } return map(fs, func(g) { return g(); }); }',
      'func forIn() { var fs = []; for (n in [1, 2, 3]) { push(fs, func() { return n; }); } return map(fs, func(g) { return g(); }); }',
      'func main() { print(cStyle()); print(forIn()); }',
    ].join('\n'),
  },
  {
    name: 'pipeline combining closures, for-in, interpolation and stdlib',
    src: 'func main() { var people = [{name: "Ada", age: 36}, {name: "Bob", age: 17}, {name: "Cy", age: 44}]; var adults = filter(people, func(p) { return p.age >= 18; }); var names = sort(map(adults, func(p) { return p.name; })); for (n in names) { print("adult: ${n}"); } print("total age ${ reduce(map(people, func(p){ return p.age; }), func(a,b){ return a+b; }) }"); }',
  },
]

// -- Module cases ------------------------------------------------------------
// Each case is a small file tree; `main.mrt` is the entry point. These verify
// that module resolution, caching, cycle detection and export binding behave
// identically in the reference interpreter and in the Playground's.
export const MODULE_CASES = [
  {
    name: 'named imports, aliasing, and nested module paths',
    files: {
      'main.mrt': 'import { PI, square, cube as cubed } from "./lib/math.mrt";\nimport { shout, VERSION } from "./lib/strings.mrt";\nfunc main() { print(PI, square(4), cubed(3)); print(shout("hi"), VERSION); }\n',
      'lib/math.mrt': 'export var PI = 3.14159;\nexport func square(n) { return n * n; }\nexport func cube(n) { return n * square(n); }\nfunc secret() { return "hidden"; }\n',
      'lib/strings.mrt': 'export func shout(s) { return toUpper(s) + "!"; }\nexport var VERSION = "1.0";\n',
    },
  },
  {
    name: 'a module is evaluated once no matter how many importers',
    files: {
      'main.mrt': 'import { a } from "./a.mrt";\nimport { b } from "./b.mrt";\nfunc main() { print(a(), b()); }\n',
      'a.mrt': 'import { tick } from "./shared.mrt";\nexport func a() { return tick(); }\n',
      'b.mrt': 'import { tick } from "./shared.mrt";\nexport func b() { return tick(); }\n',
      'shared.mrt': 'print("shared evaluated");\nvar n = 0;\nexport func tick() { n += 1; return n; }\n',
    },
  },
  {
    name: 'module top-level code runs before the entry main()',
    files: {
      'main.mrt': 'import { VALUE } from "./side.mrt";\nprint("entry top level");\nfunc main() { print("main", VALUE); }\n',
      'side.mrt': 'print("side effect");\nexport var VALUE = 42;\n',
    },
  },
  {
    name: 'private names are not importable',
    files: {
      'main.mrt': 'import { shown } from "./m.mrt";\nfunc main() { print(shown()); try { print(hidden()); } catch (e) { print(e.kind, "|", e.message); } }\n',
      'm.mrt': 'func hidden() { return "no"; }\nexport func shown() { return "yes"; }\n',
    },
  },
  {
    name: 'importing a name a module does not export',
    files: {
      'main.mrt': 'import { nothere } from "./m.mrt";\nfunc main() { }\n',
      'm.mrt': 'export var here = 1;\n',
    },
  },
  {
    name: 'a missing module reports the specifier',
    files: { 'main.mrt': 'import { x } from "./nope.mrt";\nfunc main() { }\n' },
  },
  {
    name: 'a bare specifier is rejected',
    files: { 'main.mrt': 'import { x } from "math";\nfunc main() { }\n' },
  },
  {
    name: 'circular imports are detected and named',
    files: {
      'main.mrt': 'import { a } from "./a.mrt";\nfunc main() { print(a()); }\n',
      'a.mrt': 'import { b } from "./b.mrt";\nexport func a() { return "a" + b(); }\n',
      'b.mrt': 'import { a } from "./a.mrt";\nexport func b() { return "b"; }\n',
    },
  },
  {
    name: 'imported closures keep sharing their module state',
    files: {
      'main.mrt': 'import { next, reset } from "./counter.mrt";\nfunc main() { print(next(), next()); reset(); print(next()); }\n',
      'counter.mrt': 'var n = 0;\nexport func next() { n += 1; return n; }\nexport func reset() { n = 0; }\n',
    },
  },
  {
    name: 'namespace imports bind one object of every export',
    files: {
      'main.mrt': 'import * as math from "./lib/math.mrt";\nfunc main() { print(math.PI, math.square(4)); print(keys(math)); print(type(math)); try { print(math.helper); } catch (e) { print(e.kind, "|", e.message); } }\n',
      'lib/math.mrt': 'export var PI = 3.14;\nexport func square(n) { return n * n; }\nfunc helper() { return 1; }\n',
    },
  },
  {
    name: 're-exports forward another module without binding locally',
    files: {
      'main.mrt': 'import { PI, sq, local } from "./lib/index.mrt";\nfunc main() { print(PI, sq(3), local); }\n',
      'lib/index.mrt': 'export { PI, square as sq } from "./math.mrt";\nvar local = "mine";\nexport { local };\n',
      'lib/math.mrt': 'export var PI = 3.14;\nexport func square(n) { return n * n; }\n',
    },
  },
  {
    name: 're-exporting a name the source module lacks',
    files: {
      'main.mrt': 'import { a } from "./mid.mrt";\nfunc main() { }\n',
      'mid.mrt': 'export { nope } from "./base.mrt";\n',
      'base.mrt': 'export var yes = 1;\n',
    },
  },
  {
    name: 'destructuring an imported object across module boundaries',
    files: {
      'main.mrt': 'import { config } from "./conf.mrt";\nvar {host, port = 80, ...extra} = config;\nfunc main() { print(host, port, extra); }\n',
      'conf.mrt': 'export var config = {host: "example", debug: true, region: "eu"};\n',
    },
  },
  {
    name: 'a struct declared in one module is usable from another',
    files: {
      'main.mrt': 'import { Point, origin } from "./geo.mrt";\nfunc main() { var p = Point(1, 2); print(p, p.mag() > 2); print(origin); print(type(p)); }\n',
      'geo.mrt': 'export struct Point { x, y; func mag() { return sqrt(this.x*this.x + this.y*this.y); } }\nexport var origin = Point(0, 0);\n',
    },
  },
  {
    // Two specifiers naming one file must share one evaluation, or a module
    // with side effects runs twice. Paths are normalised lexically rather
    // than canonicalised, so this is the case that pins it.
    name: 'two specifiers reaching the same file share one evaluation',
    files: {
      'main.mrt': 'import { A } from "./a.mrt";\nimport { A as B } from "./lib/../a.mrt";\nfunc main() { print(A, B); }\n',
      'a.mrt': 'print("a evaluated");\nexport var A = 1;\n',
      'lib/keep.mrt': 'export var unused = 1;\n',
    },
  },
  {
    // A specifier resolves against the file that wrote it, not against the
    // entry point -- so a module one directory down reaches back up with
    // `../`, and getting the base wrong silently resolves to the wrong file.
    name: 'a nested module resolves its own imports relative to itself',
    files: {
      'main.mrt': 'import { useA } from "./lib/inner.mrt";\nfunc main() { print(useA()); }\n',
      'lib/inner.mrt': 'import { A } from "../a.mrt";\nexport func useA() { return A + 10; }\n',
      'a.mrt': 'export var A = 1;\n',
    },
  },
  {
    name: 'a cycle three modules deep is detected and named in order',
    files: {
      'main.mrt': 'import { a } from "./a.mrt";\nfunc main() { print(a()); }\n',
      'a.mrt': 'import { b } from "./b.mrt";\nexport func a() { return "a"; }\n',
      'b.mrt': 'import { c } from "./deep/c.mrt";\nexport func b() { return "b"; }\n',
      'deep/c.mrt': 'import { a } from "../a.mrt";\nexport func c() { return "c"; }\n',
    },
  },
  {
    name: 'a module importing itself is a cycle',
    files: {
      'main.mrt': 'import { x } from "./m.mrt";\nfunc main() { print(x); }\n',
      'm.mrt': 'import { x } from "./m.mrt";\nexport var x = 1;\n',
    },
  },
  {
    // A module's scope is its own, but sits under the entry's globals: it
    // cannot leak locals out and can still see a top-level `var`.
    name: 'module scope is isolated from the importer but sees globals',
    files: {
      'main.mrt': 'var shared = "entry-global";\nimport { peek, look } from "./m.mrt";\nfunc main() { print(peek(), look()); try { print(secret); } catch (e) { print(e.kind, "|", e.message); } }\n',
      'm.mrt': 'var secret = "hidden";\nexport func peek() { return secret; }\nexport func look() { return shared; }\nfunc main() { print("module main must not run"); }\n',
    },
  },
  {
    name: 'an error in a module\'s top level propagates with its own line',
    files: {
      'main.mrt': 'import { anything } from "./boom.mrt";\nfunc main() { print("never"); }\n',
      'boom.mrt': 'var x = [1];\nprint(x[9]);\n',
    },
  },
  {
    name: 'a module with a syntax error reports it against the specifier',
    files: {
      'main.mrt': 'import { f } from "./broken.mrt";\nfunc main() { }\n',
      'broken.mrt': 'export func f( {\n',
    },
  },
  {
    name: 'importing a directory reports a missing module',
    files: {
      'main.mrt': 'import { x } from "./lib";\nfunc main() { }\n',
      'lib/thing.mrt': 'export var x = 1;\n',
    },
  },
  {
    name: 'modules combine with defaults, rest, errors and interpolation',
    files: {
      'main.mrt': 'import { describe, tally } from "./util.mrt";\nfunc main() { print(describe("x")); print(describe("x", "!")); print(tally(1, 2, 3)); try { tally(); } catch (e) { print(e.kind); } }\n',
      'util.mrt': 'export func describe(name, suffix = "?") { return "${name}${suffix}"; }\nexport func tally(first, ...rest) { return first + sum(rest); }\n',
    },
  },
]

// -- Examples outside the shared corpus --------------------------------------
// An example lands in the corpus by existing in examples/, which is what keeps
// the directory honest: every program shipped as documentation is a program all
// four implementations agree on. These are the exceptions, and each one is a
// divergence recorded rather than resolved.
//
// These examples call `ai*` or `game*`/`sound*` builtins that only the Rust
// engines have (compiler/crates/mrt-ai, compiler/crates/mrt-game,
// compiler/crates/mrt-audio). That is settled rather than outstanding: all
// are declared *engine extensions*, specified as such under "Engine
// extensions" in docs/LANGUAGE_SPEC.md. None is part of the language, so a
// program using one is not a conformance case and the Rust harnesses skip it
// rather than reporting a mismatch nobody can act on. Extensions carry their
// own tests in the crate that provides them, because being outside the
// corpus means untested otherwise.
//
// The list itself lives in scripts/engine-specific-examples.txt rather than
// here, because .github/workflows/ci.yml's "run every bundled example" step
// needs the same names and is plain bash, not JavaScript. One text file
// both can read is what keeps them from drifting apart; this export just
// wraps it in a Set for callers that already expect one.
export const ENGINE_SPECIFIC_EXAMPLES = new Set(
  readFileSync(path.join(scriptDir, 'engine-specific-examples.txt'), 'utf8')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#'))
)
