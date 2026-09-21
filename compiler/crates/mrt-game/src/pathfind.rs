//! Grid pathfinding: A* over a grid of walkable and blocked cells.
//!
//! -- Why this is a built-in, and collide is not --
//!
//! Testing whether two rectangles overlap is four lines of arithmetic. A
//! program can write it in MRT and should; an engine built-in that did
//! only that would earn nothing but a longer manual (see `collide`'s own
//! comment for the same argument made once). Finding the shortest walkable
//! route through a grid is not that: it needs a priority queue, a record
//! of the cheapest way found to each cell so far, and a search that
//! revisits a cell only when a cheaper path to it turns up. That is easy
//! to get *almost* right in an interpreted language and slow even when it
//! is exactly right, which is the reason this module exists.
//!
//! -- What it is not --
//!
//! Four-directional only, uniform cost per step. Diagonal movement raises
//! the question of whether a path may cut the corner between two blocked
//! cells, and answering it one way or the other is a decision that
//! belongs to a game's own level design, not one this module should make
//! by picking a default silently.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// A grid of cells, each walkable or not.
pub struct Grid {
    width: usize,
    height: usize,
    walkable: Vec<bool>,
}

impl std::fmt::Debug for Grid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Grid({}x{})", self.width, self.height)
    }
}

impl Grid {
    /// `walkable` is row-major, the same order `gameDrawTilemap` already
    /// reads a tile grid in: index `y * width + x`.
    pub fn new(width: usize, height: usize, walkable: Vec<bool>) -> Result<Grid, String> {
        if walkable.len() != width * height {
            return Err(format!(
                "a {width}x{height} grid needs {} cells, not {}",
                width * height,
                walkable.len()
            ));
        }
        Ok(Grid {
            width,
            height,
            walkable,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }

    pub fn is_walkable(&self, x: i32, y: i32) -> bool {
        self.contains(x, y) && self.walkable[y as usize * self.width + x as usize]
    }

    fn index(&self, x: i32, y: i32) -> usize {
        y as usize * self.width + x as usize
    }
}

/// The shortest walkable route from `start` to `goal`, inclusive of both
/// endpoints, or `None` if no route exists -- including when either end
/// is itself blocked or outside the grid.
///
/// Ties in cost are broken toward whichever state the search reached
/// first, which a plain sequence number handed to the priority queue gives
/// for free. That keeps the result deterministic call to call: two equally
/// short routes around an obstacle would otherwise depend on a heap's own
/// unspecified tie-breaking, the same nondeterminism `gameDrawTilemap`
/// avoids by drawing a sheet in a fixed row-major order rather than
/// however a hash map happened to iterate.
pub fn find_path(grid: &Grid, start: (i32, i32), goal: (i32, i32)) -> Option<Vec<(i32, i32)>> {
    if !grid.is_walkable(start.0, start.1) || !grid.is_walkable(goal.0, goal.1) {
        return None;
    }
    if start == goal {
        return Some(vec![start]);
    }

    let heuristic = |p: (i32, i32)| -> i64 {
        (p.0 - goal.0).unsigned_abs() as i64 + (p.1 - goal.1).unsigned_abs() as i64
    };

    let cells = grid.width * grid.height;
    let mut g_score = vec![i64::MAX; cells];
    let mut came_from: Vec<Option<(i32, i32)>> = vec![None; cells];
    let mut closed = vec![false; cells];

    g_score[grid.index(start.0, start.1)] = 0;

    // (f_score, a sequence number for tie-breaking, position).
    let mut open = BinaryHeap::new();
    let mut seq: u64 = 0;
    open.push(Reverse((heuristic(start), seq, start)));

    while let Some(Reverse((_, _, current))) = open.pop() {
        let current_index = grid.index(current.0, current.1);
        if closed[current_index] {
            // A cheaper route to this cell was already processed; this
            // entry is a stale duplicate left over from before that.
            continue;
        }
        closed[current_index] = true;

        if current == goal {
            let mut path = vec![goal];
            let mut at = current;
            while let Some(prev) = came_from[grid.index(at.0, at.1)] {
                path.push(prev);
                at = prev;
            }
            path.reverse();
            return Some(path);
        }

        let (x, y) = current;
        for neighbor in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
            if !grid.is_walkable(neighbor.0, neighbor.1) {
                continue;
            }
            let neighbor_index = grid.index(neighbor.0, neighbor.1);
            if closed[neighbor_index] {
                continue;
            }
            let tentative = g_score[current_index] + 1;
            if tentative < g_score[neighbor_index] {
                g_score[neighbor_index] = tentative;
                came_from[neighbor_index] = Some(current);
                seq += 1;
                open.push(Reverse((tentative + heuristic(neighbor), seq, neighbor)));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(width: usize, height: usize, blocked: &[(i32, i32)]) -> Grid {
        let mut walkable = vec![true; width * height];
        for &(x, y) in blocked {
            walkable[y as usize * width + x as usize] = false;
        }
        Grid::new(width, height, walkable).expect("valid grid")
    }

    #[test]
    fn a_grid_must_have_exactly_width_times_height_cells() {
        let message = Grid::new(3, 3, vec![true; 8]).unwrap_err();
        assert!(message.contains("needs 9 cells, not 8"), "{message}");
    }

    #[test]
    fn a_straight_line_on_an_empty_grid_is_a_straight_line() {
        let g = grid(5, 1, &[]);
        let path = find_path(&g, (0, 0), (4, 0)).expect("a path");
        assert_eq!(path, vec![(0, 0), (1, 0), (2, 0), (3, 0), (4, 0)]);
    }

    #[test]
    fn start_equals_goal_is_a_one_cell_path() {
        let g = grid(3, 3, &[]);
        assert_eq!(find_path(&g, (1, 1), (1, 1)), Some(vec![(1, 1)]));
    }

    #[test]
    fn a_route_goes_around_a_wall_rather_than_through_it() {
        // A vertical wall down the middle column, with a gap at the top.
        let g = grid(3, 3, &[(1, 1), (1, 2)]);
        let path = find_path(&g, (0, 2), (2, 2)).expect("a path exists via the gap");
        // Straight across would be 3 cells; going up to the gap, across,
        // and back down is 7 -- four extra steps for two full rows of
        // detour, not the two a diagonal shortcut would allow.
        assert_eq!(path.len(), 7);
        assert!(
            !path.contains(&(1, 1)) && !path.contains(&(1, 2)),
            "never crosses the wall"
        );
        assert!(path.contains(&(1, 0)), "goes through the gap");
    }

    #[test]
    fn a_fully_enclosed_goal_has_no_path() {
        // The goal at the centre is surrounded on all four sides.
        let g = grid(3, 3, &[(1, 0), (0, 1), (2, 1), (1, 2)]);
        assert_eq!(find_path(&g, (0, 0), (1, 1)), None);
    }

    #[test]
    fn a_blocked_start_or_goal_has_no_path() {
        let g = grid(3, 3, &[(0, 0), (2, 2)]);
        assert_eq!(find_path(&g, (0, 0), (1, 1)), None, "start is blocked");
        assert_eq!(find_path(&g, (1, 1), (2, 2)), None, "goal is blocked");
    }

    #[test]
    fn a_start_or_goal_outside_the_grid_has_no_path() {
        let g = grid(3, 3, &[]);
        assert_eq!(find_path(&g, (-1, 0), (1, 1)), None);
        assert_eq!(find_path(&g, (0, 0), (3, 3)), None);
    }

    #[test]
    fn an_unobstructed_path_is_exactly_manhattan_distance_long() {
        let g = grid(10, 10, &[]);
        let path = find_path(&g, (0, 0), (6, 3)).expect("a path");
        // 6 + 3 steps, plus the starting cell itself.
        assert_eq!(path.len(), 10);
        assert_eq!(path[0], (0, 0));
        assert_eq!(path[path.len() - 1], (6, 3));
    }

    #[test]
    fn ties_break_the_same_way_every_time() {
        // An open field: many equally short routes from corner to corner,
        // and the search must pick the same one on every call.
        let g = grid(4, 4, &[]);
        let first = find_path(&g, (0, 0), (3, 3));
        let second = find_path(&g, (0, 0), (3, 3));
        assert_eq!(first, second);
    }

    #[test]
    fn every_step_of_a_path_is_an_orthogonal_neighbour_of_the_last() {
        let g = grid(6, 6, &[(2, 0), (2, 1), (2, 2), (2, 3), (2, 4)]);
        let path = find_path(&g, (0, 0), (5, 5)).expect("a path");
        for pair in path.windows(2) {
            let (dx, dy) = (pair[1].0 - pair[0].0, pair[1].1 - pair[0].1);
            assert_eq!(dx.abs() + dy.abs(), 1, "no diagonal or teleporting steps");
        }
        assert!(grid_is_walkable_along(&g, &path));
    }

    fn grid_is_walkable_along(g: &Grid, path: &[(i32, i32)]) -> bool {
        path.iter().all(|&(x, y)| g.is_walkable(x, y))
    }
}
