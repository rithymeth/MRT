//! Boxes, and what happens when they meet.
//!
//! -- What is worth putting here, and what is not --
//!
//! Testing whether two rectangles overlap is four lines of arithmetic. A
//! program can write it in MRT and should; an engine built-in that did only
//! that would earn nothing but a longer manual.
//!
//! What a game cannot easily write for itself is the *swept* case. Move
//! something fast enough and it is on one side of a wall in one frame and the
//! other side in the next, having overlapped the wall on no frame at all --
//! so a program checking each frame's position sees nothing and the thing
//! sails through. The fix is not a better overlap test but a different
//! question: not "do these boxes overlap" but "**along this movement, when do
//! they first touch**". That is `sweep`, and it is the reason this module
//! exists.
//!
//! The other one is resolution -- given an overlap, which way to push, and
//! how far. Both are small, both are fiddly, and both are wrong in ways that
//! look like the game being haunted rather than like an error.

/// An axis-aligned box: a position and a size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Aabb {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Aabb {
        Aabb {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }
}

/// Whether two boxes share any area.
///
/// Strict: boxes that merely touch along an edge do **not** overlap. Tiles
/// laid edge to edge are the common case, and counting them as overlapping
/// would report a collision at every seam of a level.
pub fn overlap(a: &Aabb, b: &Aabb) -> bool {
    a.x < b.right() && a.right() > b.x && a.y < b.bottom() && a.bottom() > b.y
}

/// How far to move `a` to separate it from `b`, or `None` if they are apart.
///
/// The *smallest* such move, and along one axis only. Pushing out along both
/// at once would move a thing diagonally out of a wall it hit head-on, which
/// reads as the wall shoving it sideways.
pub fn resolve(a: &Aabb, b: &Aabb) -> Option<(f64, f64)> {
    if !overlap(a, b) {
        return None;
    }
    // Each axis has two ways out; take the nearer, keeping its direction.
    let right = b.right() - a.x;
    let left = a.right() - b.x;
    let push_x = if right < left { right } else { -left };

    let down = b.bottom() - a.y;
    let up = a.bottom() - b.y;
    let push_y = if down < up { down } else { -up };

    if push_x.abs() <= push_y.abs() {
        Some((push_x, 0.0))
    } else {
        Some((0.0, push_y))
    }
}

/// Where a moving box first touches a stationary one.
pub struct Hit {
    /// When, as a fraction of the movement: 0 is the start, 1 the end.
    pub time: f64,
    /// Which face was hit, pointing back at the mover: (-1, 0) for its right
    /// side striking a wall's left face. Zero on the axis that did not block.
    pub normal_x: f64,
    pub normal_y: f64,
}

/// Sweep `a` along `(dx, dy)` and report the first contact with `b`.
///
/// The slab method: each axis gives the span of movement over which the boxes
/// overlap *on that axis*, and a real collision is where those spans meet. It
/// is the same idea as a ray against a box, with the box grown by the mover's
/// size.
///
/// `None` means the movement ends without touching. Boxes already overlapping
/// when the sweep starts also give `None` -- that is not a contact along this
/// movement, it is a state, and `resolve` is what answers it.
pub fn sweep(a: &Aabb, dx: f64, dy: f64, b: &Aabb) -> Option<Hit> {
    let (x_entry, x_exit) = axis_span(a.x, a.right(), b.x, b.right(), dx);
    let (y_entry, y_exit) = axis_span(a.y, a.bottom(), b.y, b.bottom(), dy);

    let entry = x_entry.max(y_entry);
    let exit = x_exit.min(y_exit);

    // No overlap in time: the axes are never both inside at once.
    if entry > exit || entry.is_infinite() {
        return None;
    }
    // Behind the mover, or past the end of this step.
    if !(0.0..=1.0).contains(&entry) {
        return None;
    }

    // Whichever axis entered *last* is the one doing the blocking: up to that
    // moment the other axis was already overlapping and nothing stopped it.
    let (normal_x, normal_y) = if x_entry > y_entry {
        (if dx > 0.0 { -1.0 } else { 1.0 }, 0.0)
    } else {
        (0.0, if dy > 0.0 { -1.0 } else { 1.0 })
    };

    Some(Hit {
        time: entry,
        normal_x,
        normal_y,
    })
}

/// When the mover's span enters and leaves the target's, on one axis.
fn axis_span(a_min: f64, a_max: f64, b_min: f64, b_max: f64, delta: f64) -> (f64, f64) {
    if delta > 0.0 {
        ((b_min - a_max) / delta, (b_max - a_min) / delta)
    } else if delta < 0.0 {
        ((b_max - a_min) / delta, (b_min - a_max) / delta)
    } else {
        // Standing still on this axis: either the spans already overlap, and
        // do so for the whole movement, or they never will. Infinities are
        // the honest encoding -- the other axis then decides everything, and
        // `entry > exit` rules out the "never" case on its own.
        if a_max > b_min && a_min < b_max {
            (f64::NEG_INFINITY, f64::INFINITY)
        } else {
            (f64::INFINITY, f64::NEG_INFINITY)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_edges_do_not_overlap() {
        // Tiles laid edge to edge are the common case; counting them as
        // overlapping reports a collision at every seam of a level.
        let a = Aabb::new(0.0, 0.0, 10.0, 10.0);
        assert!(
            !overlap(&a, &Aabb::new(10.0, 0.0, 10.0, 10.0)),
            "right edge"
        );
        assert!(
            !overlap(&a, &Aabb::new(0.0, 10.0, 10.0, 10.0)),
            "bottom edge"
        );
        assert!(overlap(&a, &Aabb::new(9.99, 0.0, 10.0, 10.0)), "a hair in");
    }

    #[test]
    fn overlap_is_symmetric_and_handles_containment() {
        let big = Aabb::new(0.0, 0.0, 100.0, 100.0);
        let small = Aabb::new(40.0, 40.0, 5.0, 5.0);
        assert!(overlap(&big, &small));
        assert!(overlap(&small, &big), "and the other way round");
    }

    #[test]
    fn resolving_takes_the_shortest_way_out_on_one_axis() {
        // Two pixels into a wall's left face: out is two pixels left, and
        // nothing vertical. Pushing on both axes at once would slide a thing
        // sideways out of a wall it hit head-on.
        let a = Aabb::new(8.0, 5.0, 10.0, 10.0);
        let wall = Aabb::new(16.0, 0.0, 20.0, 20.0);
        let (x, y) = resolve(&a, &wall).expect("overlapping");
        assert_eq!((x, y), (-2.0, 0.0));

        // Deeper on x than on y: it comes out vertically instead.
        let b = Aabb::new(0.0, 18.0, 10.0, 10.0);
        let floor = Aabb::new(0.0, 20.0, 40.0, 10.0);
        assert_eq!(resolve(&b, &floor), Some((0.0, -8.0)));
    }

    #[test]
    fn boxes_that_are_apart_need_no_resolving() {
        let a = Aabb::new(0.0, 0.0, 5.0, 5.0);
        assert_eq!(resolve(&a, &Aabb::new(50.0, 50.0, 5.0, 5.0)), None);
        assert_eq!(
            resolve(&a, &Aabb::new(5.0, 0.0, 5.0, 5.0)),
            None,
            "touching"
        );
    }

    #[test]
    fn a_sweep_catches_what_a_frame_by_frame_check_would_miss() {
        // The whole reason this module exists. A 4-wide box at x=0 moving 200
        // in one step, against a 2-wide wall at x=100: it is clear of the
        // wall before the step and clear of it after, so testing the two
        // positions finds nothing and the thing sails through.
        let mover = Aabb::new(0.0, 0.0, 4.0, 4.0);
        let wall = Aabb::new(100.0, 0.0, 2.0, 10.0);

        let after = Aabb::new(200.0, 0.0, 4.0, 4.0);
        assert!(!overlap(&mover, &wall), "clear before");
        assert!(!overlap(&after, &wall), "clear after");

        let hit = sweep(&mover, 200.0, 0.0, &wall).expect("the sweep sees it");
        assert!((hit.time - 0.48).abs() < 1e-12, "time was {}", hit.time);
        assert_eq!((hit.normal_x, hit.normal_y), (-1.0, 0.0));

        // And the contact position is exactly touching, not inside.
        let x = mover.x + 200.0 * hit.time;
        assert!((x + mover.width - wall.x).abs() < 1e-9);
    }

    #[test]
    fn a_sweep_that_stops_short_reports_nothing() {
        let mover = Aabb::new(0.0, 0.0, 4.0, 4.0);
        let wall = Aabb::new(100.0, 0.0, 2.0, 10.0);
        assert!(sweep(&mover, 50.0, 0.0, &wall).is_none(), "ends before it");
        assert!(
            sweep(&mover, -50.0, 0.0, &wall).is_none(),
            "goes the other way"
        );
        assert!(sweep(&mover, 0.0, 0.0, &wall).is_none(), "does not move");
    }

    #[test]
    fn a_sweep_reports_the_face_it_actually_hit() {
        let wall = Aabb::new(20.0, 20.0, 20.0, 20.0);

        let from_left = Aabb::new(0.0, 25.0, 5.0, 5.0);
        let hit = sweep(&from_left, 100.0, 0.0, &wall).unwrap();
        assert_eq!((hit.normal_x, hit.normal_y), (-1.0, 0.0));

        let from_above = Aabb::new(25.0, 0.0, 5.0, 5.0);
        let hit = sweep(&from_above, 0.0, 100.0, &wall).unwrap();
        assert_eq!((hit.normal_x, hit.normal_y), (0.0, -1.0));

        let from_right = Aabb::new(60.0, 25.0, 5.0, 5.0);
        let hit = sweep(&from_right, -100.0, 0.0, &wall).unwrap();
        assert_eq!((hit.normal_x, hit.normal_y), (1.0, 0.0));
    }

    #[test]
    fn a_diagonal_sweep_blames_the_axis_that_blocked_last() {
        // Moving down and right into a box's top-left corner. Which face it
        // is depends on which axis was still outside longest, and getting it
        // backwards makes something land on a wall's side instead of sliding
        // down it.
        let wall = Aabb::new(20.0, 20.0, 40.0, 40.0);

        // Mostly downward: the top face blocks.
        let a = Aabb::new(19.0, 0.0, 5.0, 5.0);
        let hit = sweep(&a, 2.0, 100.0, &wall).unwrap();
        assert_eq!((hit.normal_x, hit.normal_y), (0.0, -1.0));

        // Mostly rightward: the left face blocks.
        let b = Aabb::new(0.0, 19.0, 5.0, 5.0);
        let hit = sweep(&b, 100.0, 2.0, &wall).unwrap();
        assert_eq!((hit.normal_x, hit.normal_y), (-1.0, 0.0));
    }

    #[test]
    fn a_sweep_that_passes_alongside_does_not_count_as_a_hit() {
        // Sliding past a wall on a parallel track: overlapping in x for the
        // whole movement, never in y. The infinities in `axis_span` are what
        // make this come out right.
        let mover = Aabb::new(0.0, 0.0, 10.0, 10.0);
        let wall = Aabb::new(0.0, 50.0, 10.0, 10.0);
        assert!(sweep(&mover, 100.0, 0.0, &wall).is_none());
    }

    #[test]
    fn boxes_already_overlapping_are_resolves_problem_not_sweeps() {
        // A sweep answers "when along this movement do they first touch". If
        // they already do, there is no such moment, and saying time 0 would
        // hide a state that needs pushing apart rather than stopping.
        let a = Aabb::new(0.0, 0.0, 10.0, 10.0);
        let b = Aabb::new(5.0, 5.0, 10.0, 10.0);
        assert!(sweep(&a, 10.0, 10.0, &b).is_none());
        assert!(resolve(&a, &b).is_some(), "but resolve has an answer");

        // And the same when it is not moving at all -- something stuck
        // inside a wall on a frame it stands still. There is no moment along
        // a movement of zero, so there is no hit; reporting one at time 0
        // would have the caller stop a thing that is already through.
        assert!(sweep(&a, 0.0, 0.0, &b).is_none());
    }

    #[test]
    fn a_sweep_landing_exactly_at_the_end_of_the_step_still_counts() {
        // time == 1.0 is a contact, not a miss: the mover ends the frame
        // touching the wall, and rejecting it would let the next frame start
        // one step inside.
        let mover = Aabb::new(0.0, 0.0, 10.0, 10.0);
        let wall = Aabb::new(20.0, 0.0, 10.0, 10.0);
        let hit = sweep(&mover, 10.0, 0.0, &wall).expect("touches at the end");
        assert_eq!(hit.time, 1.0);
    }
}
