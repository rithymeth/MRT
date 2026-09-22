"""Tests for generators: `yield`, laziness, streaming pipelines, and the
`take`/`toArray` built-ins that consume them."""

from tests.helpers import run_mrt

# -- Basics ------------------------------------------------------------------


def test_a_generator_function_is_lazy():
    output, errors = run_mrt("""
        func gen() { print("ran"); yield 1; }
        func main() {
            var g = gen();
            print("made it", type(g));
            print(toArray(g));
        }
    """)
    assert errors == []
    # Nothing in the body runs until something iterates.
    assert output == ["made it generator", "ran", "[1]"]


def test_type_and_tostring_of_generators():
    output, errors = run_mrt("""
        func named() { yield 1; }
        func main() {
            print(type(named), type(named()));
            print(toString(named()));
            var anon = func() { yield 1; };
            print(toString(anon()));
        }
    """)
    assert errors == []
    assert output == ["function generator", "<generator named>", "<generator>"]


def test_values_arrive_in_order():
    output, errors = run_mrt("""
        func countdown(start) {
            var n = start;
            while (n > 0) { yield n; n -= 1; }
            yield "liftoff";
        }
        func main() { for (v in countdown(3)) { print(v); } }
    """)
    assert errors == []
    assert output == ["3", "2", "1", "liftoff"]


def test_a_function_without_yield_is_not_a_generator():
    output, errors = run_mrt("""
        func plain() { return 1; }
        func main() { print(type(plain()), plain()); }
    """)
    assert errors == []
    assert output == ["number 1"]


def test_an_inner_function_s_yield_does_not_make_the_outer_one_a_generator():
    output, errors = run_mrt("""
        func outer() {
            var inner = func() { yield 1; };
            return inner;
        }
        func main() {
            var made = outer();
            print(type(made), type(made()));
            print(toArray(made()));
        }
    """)
    assert errors == []
    assert output == ["function generator", "[1]"]


# -- Laziness ----------------------------------------------------------------


def test_an_endless_generator_is_consumed_only_as_far_as_asked():
    output, errors = run_mrt("""
        func naturals() { var n = 0; while (true) { yield n; n += 1; } }
        func main() {
            print(take(naturals(), 5));
            var out = [];
            for (n in naturals()) {
                if (n > 4) { break; }
                push(out, n);
            }
            print(out);
        }
    """)
    assert errors == []
    assert output == ["[0, 1, 2, 3, 4]", "[0, 1, 2, 3, 4]"]


def test_generators_chain_into_streaming_pipelines():
    output, errors = run_mrt("""
        func naturals() { var n = 0; while (true) { yield n; n += 1; } }
        func squares(src) { for (x in src) { yield x * x; } }
        func takeWhile(src, pred) {
            for (x in src) { if (!pred(x)) { return; } yield x; }
        }
        func main() {
            print(toArray(takeWhile(squares(naturals()), func(v) { return v < 50; })));
        }
    """)
    assert errors == []
    assert output == ["[0, 1, 4, 9, 16, 25, 36, 49]"]


def test_return_ends_a_generator_early():
    output, errors = run_mrt("""
        func upTo(limit) {
            var n = 0;
            while (true) { if (n > limit) { return; } yield n; n += 1; }
        }
        func main() { print(toArray(upTo(3)), toArray(upTo(0))); }
    """)
    assert errors == []
    assert output == ["[0, 1, 2, 3] [0]"]


def test_generators_are_single_use():
    output, errors = run_mrt("""
        func two() { yield 1; yield 2; }
        func main() {
            var g = two();
            print(toArray(g));
            try { toArray(g); } catch (e) { print(e.kind, e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "[1, 2]",
        "ValueError <generator two> has already been iterated; "
        "a generator can only be used once.",
    ]


def test_each_call_produces_an_independent_generator():
    output, errors = run_mrt("""
        func gen(tag) { var i = 0; while (i < 2) { yield "${tag}${i}"; i += 1; } }
        func main() {
            var a = gen("a");
            var b = gen("b");
            print(toArray(a), toArray(b));
        }
    """)
    assert errors == []
    assert output == ["[a0, a1] [b0, b1]"]


def test_a_generator_keeps_its_own_scope_across_suspensions():
    output, errors = run_mrt("""
        func counter() { var n = 0; while (n < 3) { n += 1; yield n; } }
        func main() {
            var outside = 100;
            for (v in counter()) {
                outside += 1;
                print(v, outside);
            }
        }
    """)
    assert errors == []
    # The loop body reassigns its own scope between pulls; the generator's
    # `n` is unaffected.
    assert output == ["1 101", "2 102", "3 103"]


# -- yield inside every compound statement -----------------------------------


def test_yield_works_inside_every_compound_statement():
    output, errors = run_mrt("""
        func mixed() {
            for (var i = 0; i < 2; i += 1) {
                if (i == 0) { yield "if"; } else { yield "else"; }
            }
            for (c in "ab") { yield c; }
            try { yield "try"; throw "x"; }
            catch (e) { yield "catch ${e}"; }
            finally { yield "finally"; }
            match (2) { case 2: yield "match"; default: yield "no"; }
            var n = 0;
            while (n < 2) { n += 1; yield "while ${n}"; }
        }
        func main() { print(toArray(mixed())); }
    """)
    assert errors == []
    assert output == [
        "[if, else, a, b, try, catch x, finally, match, while 1, while 2]"
    ]


def test_break_and_continue_work_inside_a_generator():
    output, errors = run_mrt("""
        func g() {
            for (n in [1, 2, 3, 4, 5]) {
                if (n == 2) { continue; }
                if (n == 4) { break; }
                yield n;
            }
            yield "done";
        }
        func main() { print(toArray(g())); }
    """)
    assert errors == []
    assert output == ["[1, 3, done]"]


# -- Errors ------------------------------------------------------------------


def test_errors_propagate_out_of_a_generator_with_a_frame():
    output, errors = run_mrt("""
        func boom() { yield 1; var a = []; yield a[9]; }
        func main() {
            try { toArray(boom()); } catch (e) { print(e.kind, e.stack); }
        }
    """)
    assert errors == []
    assert output == ["IndexError [<generator boom>]"]


def test_a_throw_inside_a_generator_reaches_the_consumer():
    output, errors = run_mrt("""
        func thrower() { yield 1; throw "from generator"; }
        func main() {
            var seen = [];
            try { for (v in thrower()) { push(seen, v); } }
            catch (e) { print(seen, "caught", e); }
        }
    """)
    assert errors == []
    assert output == ["[1] caught from generator"]


def test_yield_outside_a_function_is_a_syntax_error():
    output, errors = run_mrt("yield 1;")
    assert errors != []
    assert "'yield' is only allowed inside a function" in errors[0].message


# -- take / toArray ----------------------------------------------------------


def test_take_and_toarray_work_on_every_iterable():
    output, errors = run_mrt("""
        func main() {
            print(toArray([1, 2]), toArray("ab"), toArray({x: 1, y: 2}));
            print(take([1, 2, 3], 2), take("hello", 3), take([1], 0));
        }
    """)
    assert errors == []
    assert output == ["[1, 2] [a, b] [x, y]", "[1, 2] [h, e, l] []"]


def test_take_beyond_the_end_returns_what_there_is():
    output, errors = run_mrt("""
        func two() { yield 1; yield 2; }
        func main() { print(take(two(), 10)); }
    """)
    assert errors == []
    assert output == ["[1, 2]"]


def test_take_and_toarray_reject_non_iterables():
    output, errors = run_mrt("""
        func main() {
            try { toArray(5); } catch (e) { print(e.kind, e.message); }
            try { take([1], -1); } catch (e) { print(e.kind, e.message); }
        }
    """)
    assert errors == []
    assert output == [
        "TypeError Can only iterate over an array, string, object, or generator.",
        "ValueError take() count must not be negative.",
    ]


# -- Interaction with the rest of the language -------------------------------


def test_a_struct_method_can_be_a_generator():
    output, errors = run_mrt("""
        struct Bag {
            items;
            func each() { for (x in this.items) { yield x; } }
        }
        func main() {
            var b = Bag([1, 2, 3]);
            print(toArray(b.each()), take(b.each(), 2));
        }
    """)
    assert errors == []
    assert output == ["[1, 2, 3] [1, 2]"]


def test_generators_combine_with_destructuring_and_match():
    output, errors = run_mrt("""
        func pairs() { yield [1, "a"]; yield [2, "b"]; }
        func main() {
            for ([n, s] in pairs()) { print(n, s); }
            match (take(pairs(), 1)) {
                case [[n, s]]: print("first", n, s);
                default: print("no");
            }
        }
    """)
    assert errors == []
    assert output == ["1 a", "2 b", "first 1 a"]
