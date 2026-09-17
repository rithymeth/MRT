"""Tests for the iterator protocol: a struct that declares `iter()` is
iterable everywhere an array or generator is."""

from tests.helpers import run_mrt

SPAN = '''
struct Span {
    lo, hi;
    func iter() { var n = this.lo; while (n < this.hi) { yield n; n += 1; } }
}
'''


def test_a_struct_with_iter_works_in_a_for_in_loop():
    output, errors = run_mrt(SPAN + '''
        func main() { for (v in Span(1, 4)) { print(v); } }
    ''')
    assert errors == []
    assert output == ["1", "2", "3"]


def test_it_reaches_toarray_take_and_the_higher_order_builtins():
    output, errors = run_mrt(SPAN + '''
        func main() {
            var s = Span(0, 4);
            print(toArray(s), take(Span(0, 99), 3));
            print(map(s, func(n) { return n * 2; }));
            print(filter(s, func(n) { return n % 2 == 0; }));
            print(reduce(Span(1, 5), func(a, b) { return a + b; }));
            print(find(s, func(n) { return n > 2; }),
                  some(s, func(n) { return n > 9; }),
                  every(s, func(n) { return n < 9; }));
        }
    ''')
    assert errors == []
    assert output == ["[0, 1, 2, 3] [0, 1, 2]", "[0, 2, 4, 6]", "[0, 2]", "10",
                      "3 false true"]


def test_each_iteration_calls_iter_afresh():
    output, errors = run_mrt(SPAN + '''
        func main() {
            var s = Span(0, 2);
            print(toArray(s), toArray(s), toArray(s));
        }
    ''')
    assert errors == []
    # Unlike a generator, a struct is not consumed by being iterated: `iter()`
    # is called again each time and hands back a fresh sequence.
    assert output == ["[0, 1] [0, 1] [0, 1]"]


def test_iter_may_return_any_iterable():
    output, errors = run_mrt('''
        struct Deck { cards; func iter() { return this.cards; } }
        struct Word { text; func iter() { return this.text; } }
        func main() { print(toArray(Deck(["A", "K"])), toArray(Word("hi"))); }
    ''')
    assert errors == []
    assert output == ["[A, K] [h, i]"]


def test_a_struct_without_iter_still_iterates_its_field_names():
    output, errors = run_mrt('''
        struct Plain { a, b }
        func main() { for (k in Plain(1, 2)) { print(k); } }
    ''')
    assert errors == []
    assert output == ["a", "b"]


def test_a_bad_iter_is_reported_against_the_struct():
    output, errors = run_mrt('''
        struct Loop { n; func iter() { return this; } }
        struct Bad { n; func iter() { return this.n; } }
        func main() {
            try { for (x in Loop(1)) { print(x); } } catch (e) { print(e.kind, e.message); }
            try { toArray(Bad(5)); } catch (e) { print(e.kind, e.message); }
        }
    ''')
    assert errors == []
    assert output == [
        "ValueError Loop.iter() returned the struct itself, "
        "which would iterate forever.",
        "TypeError Bad.iter() returned number, which isn't iterable.",
    ]


def test_iter_can_be_lazy_and_endless():
    output, errors = run_mrt('''
        struct Counter { start; func iter() { var n = this.start; while (true) { yield n; n += 1; } } }
        func main() { print(take(Counter(10), 4)); }
    ''')
    assert errors == []
    assert output == ["[10, 11, 12, 13]"]
