//! A pool of short-lived points: sparks, smoke, an explosion's debris.
//!
//! -- Why a pool, not one object per burst --
//!
//! A program that wants a hundred sparks from one hit does not want to
//! manage a hundred ids, the way it would for a hundred sprites -- there is
//! nothing to draw once, stamp many times, or address individually. What it
//! wants is to say "here, and moving this way, and gone in this long" a
//! hundred times and then forget about each one. So this crate holds exactly
//! one pool, spawned into and drawn as a whole, the same shape the camera and
//! the current draw target already take: state the screen carries, not an id
//! a program has to keep.
//!
//! -- Why fading rather than a fixed size or a fixed count --
//!
//! A burst that blinks out all at once at the end of its life reads as a
//! bug, not an effect. Scaling each particle's own alpha by how much of its
//! life remains is what makes a hundred sparks read as *dissipating*
//! together, for the cost of one multiply per particle per frame.

use crate::{Color, Surface};

/// One live particle: a point with a velocity and a remaining lifetime.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    /// Seconds left to live.
    pub life: f64,
    /// `life` at the moment this particle was spawned, kept so `draw` can
    /// tell how much of its life is left as a fraction rather than only how
    /// many seconds -- the fraction is what the fade actually needs.
    pub max_life: f64,
    pub color: Color,
}

/// A pool of particles, all spawned into and updated together.
#[derive(Default)]
pub struct Particles {
    items: Vec<Particle>,
}

impl Particles {
    pub fn new() -> Particles {
        Particles::default()
    }

    /// Add one particle at `(x, y)`, moving at `(vx, vy)` pixels a second,
    /// living for `life` seconds.
    ///
    /// A non-positive or non-finite `life` spawns nothing, the same "not an
    /// error, just nothing to show" rule a negative box size gets elsewhere
    /// in this crate: a particle already dead the instant it exists is not
    /// worth a slot in the pool.
    pub fn spawn(&mut self, x: f64, y: f64, vx: f64, vy: f64, life: f64, color: Color) {
        if !life.is_finite() || life <= 0.0 {
            return;
        }
        self.items.push(Particle {
            x,
            y,
            vx,
            vy,
            life,
            max_life: life,
            color,
        });
    }

    /// Move every particle by `dt` seconds of its own velocity and age it by
    /// the same amount, dropping whichever ones have run out of life.
    pub fn update(&mut self, dt: f64) {
        for p in &mut self.items {
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.life -= dt;
        }
        self.items.retain(|p| p.life > 0.0);
    }

    /// How many particles are alive right now.
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Draw every live particle onto `surface` as a single pixel, offset by
    /// `(offset_x, offset_y)` and its own colour faded toward transparent in
    /// proportion to how much of its life is left.
    ///
    /// The offset is what lets a caller with its own idea of a camera --
    /// this crate has none -- scroll particles along with everything else a
    /// program draws, without this module needing to know what a camera is.
    ///
    /// One pixel rather than a shape: a particle is meant to be spawned by
    /// the hundred, and this crate already has `fill_rect`/`draw_circle` for
    /// a program that wants one shape drawn carefully and few of them.
    pub fn draw(&self, surface: &mut Surface, offset_x: i32, offset_y: i32) {
        for p in &self.items {
            let fraction = (p.life / p.max_life).clamp(0.0, 1.0);
            let faded = Color::rgba(
                p.color.red(),
                p.color.green(),
                p.color.blue(),
                (p.color.alpha() as f64 * fraction).round() as u8,
            );
            surface.draw(
                p.x.floor() as i32 + offset_x,
                p.y.floor() as i32 + offset_y,
                faded,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BLACK, WHITE};

    #[test]
    fn a_particle_with_non_positive_or_non_finite_life_is_never_spawned() {
        let mut particles = Particles::new();
        particles.spawn(0.0, 0.0, 0.0, 0.0, 0.0, WHITE);
        particles.spawn(0.0, 0.0, 0.0, 0.0, -1.0, WHITE);
        particles.spawn(0.0, 0.0, 0.0, 0.0, f64::NAN, WHITE);
        particles.spawn(0.0, 0.0, 0.0, 0.0, f64::INFINITY, WHITE);
        assert_eq!(particles.count(), 0);
    }

    #[test]
    fn updating_moves_a_particle_by_its_own_velocity_times_dt() {
        let mut particles = Particles::new();
        particles.spawn(10.0, 20.0, 2.0, -3.0, 5.0, WHITE);
        particles.update(0.5);
        assert_eq!(particles.items[0].x, 11.0);
        assert_eq!(particles.items[0].y, 18.5);
    }

    #[test]
    fn a_particle_dies_once_its_life_runs_out() {
        let mut particles = Particles::new();
        particles.spawn(0.0, 0.0, 0.0, 0.0, 1.0, WHITE);
        particles.update(0.6);
        assert_eq!(particles.count(), 1, "still has a little life left");
        particles.update(0.6);
        assert_eq!(particles.count(), 0, "used up its life and is gone");
    }

    #[test]
    fn drawing_fades_a_particle_toward_transparent_as_it_ages() {
        // Drawing blends over what is there, the same as everything else in
        // this crate -- so a fresher (more opaque) white particle mixes in
        // more white over a black background than a nearly-dead (more
        // transparent) one does. The alpha byte of the *result* cannot show
        // this: compositing anything over an already-opaque background is
        // still opaque, by definition, no matter how transparent the thing
        // drawn on top was. The RGB channels are where the fade actually
        // shows up.
        let mut particles = Particles::new();
        particles.spawn(2.0, 2.0, 0.0, 0.0, 1.0, WHITE);

        let mut fresh = Surface::new(5, 5, BLACK);
        particles.draw(&mut fresh, 0, 0);
        let fresh_red = fresh.get(2, 2).unwrap().red();
        assert_eq!(fresh_red, 255, "full life, full colour");

        particles.update(0.75);
        let mut aged = Surface::new(5, 5, BLACK);
        particles.draw(&mut aged, 0, 0);
        let aged_red = aged.get(2, 2).unwrap().red();
        assert!(
            aged_red < fresh_red,
            "a quarter of its life left should be fainter, not {aged_red}"
        );
        assert!(
            aged_red > 0,
            "still alive, so still at least a little visible"
        );
    }

    #[test]
    fn drawing_with_an_offset_shifts_where_a_particle_lands() {
        let mut particles = Particles::new();
        particles.spawn(1.0, 1.0, 0.0, 0.0, 1.0, WHITE);
        let mut surface = Surface::new(5, 5, BLACK);
        particles.draw(&mut surface, 2, 3);
        assert_eq!(surface.get(3, 4), Some(WHITE), "shifted by the offset");
        assert_eq!(
            surface.get(1, 1),
            Some(BLACK),
            "not where it would land with no offset"
        );
    }

    #[test]
    fn a_dead_particle_draws_nothing() {
        let mut particles = Particles::new();
        particles.spawn(2.0, 2.0, 0.0, 0.0, 0.1, WHITE);
        particles.update(1.0);
        let mut surface = Surface::new(5, 5, BLACK);
        particles.draw(&mut surface, 0, 0);
        assert_eq!(surface.get(2, 2), Some(BLACK), "nothing left to draw");
    }
}
