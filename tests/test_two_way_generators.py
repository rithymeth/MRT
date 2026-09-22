"""Tests for the two-way half of generators: `yield` as a value, the
`next`/`send` drive protocol, `yield*` delegation, resumability, and the lazy
`map`/`filter` built on top of them."""

from tests.helpers import run_mrt

# -- yield as a value --------------------------------------------------------


def test_a_generator_receives_the_value_sent_in():
    output, errors = run_mrt("""
        func echo() {
            var got = yield "ready";
            while (got != "stop") { got = yield "saw ${got}"; }
            yield "bye";
        }
        func main() {
            var g = echo();
            print(next(g));
            print(send(g, "a"));
            print(send(g, "b"));
            print(send(g, "stop"));
            print(next(g));
        }
    """)
    assert errors == []
    assert output == [
        "{done: false, value: ready}",
        "{done: false, value: saw a}",
        "{done: false, value: saw b}",
        "{done: false, value: bye}",
        "{done: true, value: null}",
    ]


def test_the_value_sent_on_the_first_step_is_discarded():
    output, errors = run_mrt("""
        func g() { var first = yield 1; print("got", first); }
        func main() { var it = g(); send(it, "ignored"); send(it, "kept"); }
    """)
    assert errors == []
    # The body hasn't reached a `yield` yet on the first step, so there is
    # nowhere for "ignored" to land.
    assert output == ["got kept"]


def test_a_yield_with_no_sender_produces_null():
    output, errors = run_mrt("""
        func g() { var a = yield 1; print("a", a); }
        func main() { print(toArray(g())); }
    """)
    assert errors == []
    assert output == ["a null", "[1]"]


def test_a_sent_value_lands_in_every_assignment_shape():
    output, errors = run_mrt("""
        func g() {
            var declared = yield 1;
            var plain = 0;
            plain = yield 2;
            var o = {k: 0};
            o.k = yield 3;
            var a = 0; var b = 0;
            [a, b] = yield 4;
            print(declared, plain, o.k, a, b);
        }
        func main() {
            var it = g();
            next(it);
            send(it, "one"); send(it, "two"); send(it, "three"); send(it, [4, 5]);
        }
    """)
    assert errors == []
    assert output == ["one two three 4 5"]


def test_yield_outside_the_allowed_shapes_is_a_syntax_error():
    for source in [
        "func f() { print(1 + yield 2); } func main() { f(); }",
        "func f() { var xs = [yield 1]; } func main() { f(); }",
        "func f() { var x = 0; x += yield 1; } func main() { f(); }",
    ]:
        output, errors = run_mrt(source)
        assert errors != [], source
        assert "Expect expression" in errors[0].message


# -- yield* ------------------------------------------------------------------


def test_yield_star_delegates_to_any_iterable():
    output, errors = run_mrt("""
        func inner() { yield 1; yield 2; }
        func outer() {
            yield 0;
            yield* inner();
            yield* [8, 9];
            yield* "ab";
            yield* {k: 1};
            yield 3;
        }
        func main() { print(toArray(outer())); }
    """)
    assert errors == []
    assert output == ["[0, 1, 2, 8, 9, a, b, k, 3]"]


def test_yield_star_forwards_sent_values_into_the_delegate():
    output, errors = run_mrt("""
        func inner() {
            var a = yield "i1";
            var b = yield "i2";
            print("inner saw", a, b);
        }
        func outer() { yield "o"; yield* inner(); }
        func main() {
            var g = outer();
            next(g); send(g, "x"); send(g, "y"); send(g, "z");
        }
    """)
    assert errors == []
    # "x" resumes `outer`'s plain `yield`, which has nowhere to put it; "y"
    # and "z" reach the delegate.
    assert output == ["inner saw y z"]


def test_yield_star_is_a_statement_only():
    output, errors = run_mrt("func f() { var x = yield* [1]; } func main() { f(); }")
    assert errors != []
    assert "has no value of its own" in errors[0].message


def test_delegation_nests():
    output, errors = run_mrt("""
        func a() { yield 1; }
        func b() { yield* a(); yield 2; }
        func c() { yield* b(); yield 3; }
        func main() { print(toArray(c())); }
    """)
    assert errors == []
    assert output == ["[1, 2, 3]"]


# -- Resumability ------------------------------------------------------------


def test_a_generator_resumes_where_the_previous_consumer_stopped():
    output, errors = run_mrt("""
        func naturals() { var n = 0; while (true) { yield n; n += 1; } }
        func main() {
            var g = naturals();
            print(take(g, 3));
            print(take(g, 2));
            for (v in g) { if (v > 6) { break; } }
            print(next(g));
        }
    """)
    assert errors == []
    assert output == ["[0, 1, 2]", "[3, 4]", "{done: false, value: 8}"]


def test_take_pulls_exactly_as_many_items_as_asked():
    output, errors = run_mrt("""
        func three() { yield 1; yield 2; yield 3; }
        func main() { var g = three(); print(take(g, 0)); print(take(g, 2)); print(take(g, 9)); }
    """)
    assert errors == []
    assert output == ["[]", "[1, 2]", "[3]"]


def test_an_exhausted_generator_is_an_error_to_iterate_but_not_to_step():
    output, errors = run_mrt("""
        func two() { yield 1; yield 2; }
        func main() {
            var g = two();
            print(toArray(g));
            try { toArray(g); } catch (e) { print(e.kind); }
            print(next(g));
        }
    """)
    assert errors == []
    # A drive loop ends naturally on `done`; a `for`-`in` over a finished
    # generator is almost always a mistake, so it still raises.
    assert output == ["[1, 2]", "ValueError", "{done: true, value: null}"]


def test_resuming_a_generator_from_inside_itself_is_reported():
    output, errors = run_mrt("""
        var shared = null;
        func f() { yield 1; var again = next(shared); yield 2; }
        func main() {
            shared = f();
            try { toArray(shared); } catch (e) { print(e.kind, e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "ValueError <generator f> is already running; "
        "a generator can't be resumed from inside itself."
    ]


def test_next_and_send_reject_non_generators():
    output, errors = run_mrt("""
        func main() {
            try { next(5); } catch (e) { print(e.kind, e.message); }
            try { send([1], 2); } catch (e) { print(e.kind, e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "TypeError next() needs a generator, not number.",
        "TypeError send() needs a generator, not array.",
    ]


# -- Lazy map / filter -------------------------------------------------------


def test_map_and_filter_stay_lazy_over_a_generator():
    output, errors = run_mrt("""
        func naturals() { var n = 0; while (true) { yield n; n += 1; } }
        func main() {
            print(type(map(naturals(), func(n) { return n; })));
            print(take(map(naturals(), func(n) { return n * n; }), 5));
            print(take(filter(naturals(), func(n) { return n % 2 == 0; }), 4));
            print(take(map(filter(naturals(), func(n) { return n % 3 == 0; }),
                           func(n) { return "n${n}"; }), 3));
        }
    """)
    assert errors == []
    assert output == [
        "generator",
        "[0, 1, 4, 9, 16]",
        "[0, 2, 4, 6]",
        "[n0, n3, n6]",
    ]


def test_a_lazy_map_only_calls_its_function_as_far_as_asked():
    output, errors = run_mrt("""
        func three() { yield 1; yield 2; yield 3; }
        func main() {
            var calls = 0;
            var doubled = map(three(), func(x) { calls += 1; return x * 2; });
            print(calls);
            print(take(doubled, 2), calls);
            print(toArray(doubled), calls);
        }
    """)
    assert errors == []
    assert output == ["0", "[2, 4] 2", "[6] 3"]


def test_map_and_filter_are_eager_on_everything_else():
    output, errors = run_mrt("""
        func main() {
            print(map([1, 2], func(x) { return x + 1; }));
            print(filter([1, 2, 3], func(x) { return x > 1; }));
            print(map("abc", func(c) { return toUpper(c); }));
            print(map({a: 1, b: 2}, func(k) { return k + "!"; }));
        }
    """)
    assert errors == []
    assert output == ["[2, 3]", "[2, 3]", "[A, B, C]", "[a!, b!]"]


def test_the_eager_higher_order_builtins_reject_non_iterables():
    output, errors = run_mrt("""
        func main() {
            try { map(5, func(x) { return x; }); } catch (e) { print(e.kind, e.message); }
            try { reduce(7, func(a, b) { return a; }); } catch (e) { print(e.kind, e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "TypeError map() needs something iterable, not number.",
        "TypeError reduce() needs something iterable, not number.",
    ]


# -- Abandoned generators ----------------------------------------------------
#
# A suspended generator that nothing refers to any more is garbage. CPython
# disposes of one by throwing GeneratorExit at its `yield`, which happens at a
# collection point -- an arbitrary moment during unrelated execution. Cleanup
# written as `try`/`finally` inside the interpreter therefore ran in the middle
# of someone else's work, and quietly replaced the live scope with a stale one.
#
# These tests exist because a benchmark, not a test, found it: the failure
# needed a few hundred abandoned generators before a collection happened at an
# unlucky moment.


def test_abandoning_many_generators_does_not_corrupt_the_interpreter():
    output, errors = run_mrt("""
        func naturals() { var n = 0; while (true) { yield n; n += 1; } }
        func main() {
            var total = 0;
            var rounds = 0;
            while (rounds < 300) {
                var g = naturals();
                total += len(take(g, 3));
                rounds += 1;
            }
            print(total);
        }
    """)
    assert errors == []
    # Before the fix this failed with "Undefined variable 'n'", pointing at a
    # variable plainly in scope, after however many rounds it took for the
    # garbage collector to run.
    assert output == ["900"]


def test_an_abandoned_generator_does_not_run_its_finally():
    output, errors = run_mrt("""
        func g() {
            try { yield 1; yield 2; }
            finally { print("cleanup"); }
        }
        func main() {
            var rounds = 0;
            while (rounds < 300) { take(g(), 1); rounds += 1; }
            print("done");
        }
    """)
    assert errors == []
    # Dropping a generator runs nothing. There is no point in the program at
    # which the cleanup could be said to happen, and the JavaScript
    # implementation cannot run it at all, so "invisible" is the only
    # behaviour both implementations can agree on.
    assert output == ["done"]


def test_a_finally_still_runs_when_the_generator_actually_finishes():
    output, errors = run_mrt("""
        func g() {
            try { yield 1; yield 2; }
            finally { print("cleanup"); }
        }
        func main() { print(toArray(g())); }
    """)
    assert errors == []
    assert output == ["cleanup", "[1, 2]"]


def test_abandoning_a_generator_mid_loop_leaves_the_caller_intact():
    output, errors = run_mrt("""
        func naturals() { var n = 0; while (true) { yield n; n += 1; } }
        func main() {
            var sum = 0;
            var i = 0;
            while (i < 300) {
                var local = i * 2;
                for (v in naturals()) { if (v > 1) { break; } }
                sum += local;
                i += 1;
            }
            print(sum);
        }
    """)
    assert errors == []
    # `local` must still be readable after each abandoned for-in.
    assert output == ["89700"]
