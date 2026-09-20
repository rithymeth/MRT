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

/// A circle: a centre and a radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
}

impl Circle {
    pub fn new(x: f64, y: f64, radius: f64) -> Circle {
        Circle { x, y, radius }
    }
}

/// Whether two circles share any area.
///
/// Strict, the same rule `overlap` follows for boxes: circles that merely
/// touch do not overlap.
pub fn circle_overlap(a: &Circle, b: &Circle) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let r = a.radius + b.radius;
    dx * dx + dy * dy < r * r
}

/// How far to move `a` to separate it from `b`, or `None` if they are apart.
///
/// Pushed straight along the line between the two centres -- the only
/// direction that separates two circles without turning either into an
/// ellipse. Two circles that share an exact centre have no such line to
/// follow; `(1, 0)` is picked for them arbitrarily; the whole point of the
/// gap between them is unrepresentable, some direction has to be chosen and
/// none is more correct than another.
pub fn circle_resolve(a: &Circle, b: &Circle) -> Option<(f64, f64)> {
    if !circle_overlap(a, b) {
        return None;
    }
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    let distance = (dx * dx + dy * dy).sqrt();
    let push = a.radius + b.radius - distance;
    if distance > 0.0 {
        Some((dx / distance * push, dy / distance * push))
    } else {
        Some((push, 0.0))
    }
}

/// Whether a circle and a box share any area.
///
/// Clamping the circle's centre into the box's own bounds gives the closest
/// point on the box to that centre; once the centre is already inside the
/// box, that closest point *is* the centre, at distance zero, which counts
/// as overlapping with no separate case needed for it.
pub fn circle_box_overlap(c: &Circle, b: &Aabb) -> bool {
    let (dx, dy) = (
        c.x - c.x.clamp(b.x, b.right()),
        c.y - c.y.clamp(b.y, b.bottom()),
    );
    dx * dx + dy * dy < c.radius * c.radius
}

/// How far to move the circle to separate it from the box, or `None`.
///
/// Outside the box, this pushes along the line from the box's nearest point
/// to the circle's centre -- the same idea as two circles, with that
/// nearest point standing in for the other circle's own centre. A centre
/// already inside the box has no such point to push away from -- every
/// point on the box is equally "at" it -- so that case instead pushes out
/// through whichever face is nearest, the same shortest-way-out choice
/// `resolve` makes between two boxes.
pub fn circle_box_resolve(c: &Circle, b: &Aabb) -> Option<(f64, f64)> {
    if !circle_box_overlap(c, b) {
        return None;
    }
    let (closest_x, closest_y) = (c.x.clamp(b.x, b.right()), c.y.clamp(b.y, b.bottom()));
    let (dx, dy) = (c.x - closest_x, c.y - closest_y);
    let distance = (dx * dx + dy * dy).sqrt();
    if distance > 0.0 {
        let push = c.radius - distance;
        return Some((dx / distance * push, dy / distance * push));
    }
    let left = c.x - b.x;
    let right = b.right() - c.x;
    let top = c.y - b.y;
    let bottom = b.bottom() - c.y;
    if left <= right && left <= top && left <= bottom {
        Some((-(left + c.radius), 0.0))
    } else if right <= top && right <= bottom {
        Some((right + c.radius, 0.0))
    } else if top <= bottom {
        Some((0.0, -(top + c.radius)))
    } else {
        Some((0.0, bottom + c.radius))
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

    #[test]
    fn circles_overlap_when_closer_than_the_sum_of_their_radii() {
        let a = Circle::new(0.0, 0.0, 5.0);
        assert!(circle_overlap(&a, &Circle::new(9.0, 0.0, 5.0)), "a hair in");
        assert!(
            !circle_overlap(&a, &Circle::new(10.0, 0.0, 5.0)),
            "touching edges do not overlap, same rule as boxes"
        );
        assert!(!circle_overlap(&a, &Circle::new(20.0, 0.0, 5.0)), "apart");
    }

    #[test]
    fn resolving_circles_pushes_along_the_line_between_their_centres() {
        // Centres 6 apart, radii summing to 10: 4 units of overlap, pushed
        // straight back along the line joining them -- here the x axis, so
        // all 4 units land on x.
        let a = Circle::new(0.0, 0.0, 5.0);
        let b = Circle::new(6.0, 0.0, 5.0);
        let (x, y) = circle_resolve(&a, &b).expect("overlapping");
        assert!((x - -4.0).abs() < 1e-9, "x = {x}");
        assert!(y.abs() < 1e-9, "y = {y}");

        // Moving `a` by that push should leave the two circles exactly
        // touching rather than overlapping or gapped.
        let moved = Circle::new(a.x + x, a.y + y, a.radius);
        assert!(!circle_overlap(&moved, &b), "no longer overlapping");
        let (dx, dy) = (moved.x - b.x, moved.y - b.y);
        let distance = (dx * dx + dy * dy).sqrt();
        assert!(
            (distance - 10.0).abs() < 1e-9,
            "exactly touching: {distance}"
        );
    }

    #[test]
    fn circles_sharing_a_centre_still_resolve_to_something() {
        // No line between two identical centres to push along -- (1, 0) is
        // picked arbitrarily, but the *distance* pushed must still fully
        // separate the two circles.
        let a = Circle::new(3.0, 3.0, 4.0);
        let b = Circle::new(3.0, 3.0, 6.0);
        let (x, y) = circle_resolve(&a, &b).expect("overlapping");
        assert_eq!(y, 0.0);
        assert_eq!(x, 10.0);
    }

    #[test]
    fn circles_that_are_apart_need_no_resolving() {
        let a = Circle::new(0.0, 0.0, 1.0);
        let b = Circle::new(100.0, 100.0, 1.0);
        assert_eq!(circle_resolve(&a, &b), None);
    }

    #[test]
    fn a_circle_overlaps_a_box_it_is_not_touching_but_is_close_to() {
        let box_ = Aabb::new(0.0, 0.0, 10.0, 10.0);
        // Nearest point on the box to (15, 15) is its corner (10, 10),
        // distance sqrt(50) =~ 7.07 -- inside a radius of 8, outside 7.
        assert!(circle_box_overlap(&Circle::new(15.0, 15.0, 8.0), &box_));
        assert!(!circle_box_overlap(&Circle::new(15.0, 15.0, 7.0), &box_));
    }

    #[test]
    fn a_circle_centred_inside_a_box_overlaps_it_with_no_special_case() {
        let box_ = Aabb::new(0.0, 0.0, 10.0, 10.0);
        assert!(circle_box_overlap(&Circle::new(5.0, 5.0, 0.1), &box_));
    }

    #[test]
    fn resolving_a_circle_outside_a_box_pushes_along_the_line_to_its_centre() {
        let box_ = Aabb::new(0.0, 0.0, 10.0, 10.0);
        // Straight out to the right of the box, overlapping by 2.
        let c = Circle::new(15.0, 5.0, 7.0);
        let (x, y) = circle_box_resolve(&c, &box_).expect("overlapping");
        assert!((x - 2.0).abs() < 1e-9, "x = {x}");
        assert!(y.abs() < 1e-9, "y = {y}");
    }

    #[test]
    fn resolving_a_circle_centred_inside_a_box_pushes_out_the_nearest_face() {
        let box_ = Aabb::new(0.0, 0.0, 10.0, 100.0);
        // Nearer the left face (2 in) than any other, so it should come out
        // to the left, clearing the box by exactly its own radius.
        let c = Circle::new(2.0, 50.0, 3.0);
        let (x, y) = circle_box_resolve(&c, &box_).expect("overlapping");
        assert_eq!((x, y), (-5.0, 0.0));
        assert_eq!(c.x + x, -3.0, "now clear of the box by exactly the radius");
    }

    #[test]
    fn a_circle_and_box_that_are_apart_need_no_resolving() {
        let box_ = Aabb::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            circle_box_resolve(&Circle::new(100.0, 100.0, 1.0), &box_),
            None
        );
    }
}
