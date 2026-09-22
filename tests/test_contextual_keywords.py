"""`from` and `as` only mean anything inside an import or export clause, so a
program may use them as ordinary names."""

from tests.helpers import run_mrt


def test_from_and_as_work_as_variables_and_parameters():
    output, errors = run_mrt("""
        func trip(from, as) { return "${from}->${as}"; }
        func main() {
            var from = "here"; var as = "now";
            print(from, as);
            print(trip("A", "B"));
        }
    """)
    assert errors == []
    assert output == ["here now", "A->B"]


def test_from_and_as_work_as_object_keys_and_struct_fields():
    output, errors = run_mrt("""
        struct Trip { from, to }
        func main() {
            var o = {from: 1, as: 2, to: 3};
            print(o, o.from, o.as);
            var t = Trip("A", "B");
            print(t, t.from);
            var {from: f} = o;
            print(f);
        }
    """)
    assert errors == []
    assert output == ["{from: 1, as: 2, to: 3} 1 2", "Trip(from: A, to: B) A", "1"]


def test_from_and_as_work_as_loop_variables_and_match_bindings():
    output, errors = run_mrt("""
        func main() {
            for (from in [1, 2]) { print(from); }
            print(match (9) { case as: as + 1, default: 0 });
        }
    """)
    assert errors == []
    assert output == ["1", "2", "10"]
