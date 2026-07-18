//! Involute spur gear geometry generation.
//!
//! A correct involute gear tooth profile requires three things the naive
//! implementation gets wrong:
//!
//! 1. **Involute angle compensation** — a point on the involute at radius `r`
//!    sits at angle `inv(α_r) = tan(α_r) − α_r` from the involute's base point.
//!    To make the tooth thickness exactly half the angular pitch at the pitch
//!    circle, the flank must be rotated by `±(π/2z) ± inv(α)`, not just `±π/2z`.
//!
//! 2. **Flank lean direction** — the left flank unwinds CCW and the right
//!    flank unwinds CW, so both lean *toward the tooth centerline* as radius
//!    increases. Teeth get thinner toward the tip; getting this backwards
//!    produces teeth that are wider at the tip than at the root.
//!
//! 3. **Closed-loop topology** — the profile must be a single cycle of
//!    shared points. Duplicating the first point at the end creates a
//!    coincident-but-distinct entity, which breaks closed-loop detection
//!    during extrusion.

use echi_core::sketch::{EntityId, Sketch};
use std::f64::consts::PI;

/// Parameters defining a spur gear.
pub struct GearParams {
    /// Module (tooth size), mm.
    pub module: f64,
    /// Number of teeth.
    pub teeth: u32,
    /// Pressure angle in degrees (typically 20).
    pub pressure_angle_deg: f64,
    /// Addendum coefficient (typically 1.0).
    pub addendum_coef: f64,
    /// Dedendum coefficient (typically 1.25).
    pub dedendum_coef: f64,
    /// Number of sampled points per involute flank.
    pub involute_points: u32,
}

impl Default for GearParams {
    fn default() -> Self {
        Self {
            module: 2.0,
            teeth: 20,
            pressure_angle_deg: 20.0,
            addendum_coef: 1.0,
            dedendum_coef: 1.25,
            involute_points: 10,
        }
    }
}

fn polar(r: f64, a: f64) -> (f64, f64) {
    (r * a.cos(), r * a.sin())
}

/// inv(x) = tan(x) − x, the involute function.
fn inv(x: f64) -> f64 {
    x.tan() - x
}

/// Build a closed polyline sketch from profile points: one point entity per
/// coordinate, lines between consecutive points, and a final line from the
/// last point back to the FIRST point (true closed loop, no duplicates).
pub fn closed_loop_sketch(profile: &[(f64, f64)]) -> Sketch {
    let mut sketch = Sketch::new();
    let n = profile.len();
    let mut ids: Vec<EntityId> = Vec::with_capacity(n);
    for (x, y) in profile {
        ids.push(sketch.add_point(*x, *y));
    }
    for i in 0..n {
        sketch.add_line(ids[i], ids[(i + 1) % n]);
    }
    sketch
}

/// Generate a complete involute spur gear profile as a Sketch.
///
/// The gear is centered at origin. Tooth 0 is centered on the +x axis.
/// Returns a single closed loop of line entities plus a center reference point.
pub fn generate_gear_sketch(params: &GearParams) -> Sketch {
    let m = params.module;
    let z = params.teeth as f64;
    let alpha = params.pressure_angle_deg.to_radians();

    // Key radii
    let rp = m * z / 2.0; // pitch
    let ra = rp + params.addendum_coef * m; // tip
    let rd = rp - params.dedendum_coef * m; // root
    let rb = rp * alpha.cos(); // base circle

    let inv_a = inv(alpha);
    // The involute can't extend below the base circle. If the root circle is
    // below the base circle, the flank starts at rb and connects down to rd
    // with a radial segment; otherwise it starts at rd.
    let r_start = rb.max(rd);
    let alpha_start = (rb / r_start).acos();
    let inv_start = inv(alpha_start);
    let alpha_a = (rb / ra).acos();
    let inv_a_tip = inv(alpha_a);

    let half_tooth = PI / (2.0 * z); // half tooth-thickness angle at pitch
    let pitch = 2.0 * PI / z; // angular pitch per tooth

    let n_flank = params.involute_points.max(3) as usize;
    let n_tip = 3usize;
    let n_root = 3usize;

    let mut profile: Vec<(f64, f64)> = Vec::new();
    // Push a point, skipping exact duplicates of the previous one.
    let mut push = |p: (f64, f64)| {
        if let Some(last) = profile.last() {
            if (last.0 - p.0).abs() < 1e-10 && (last.1 - p.1).abs() < 1e-10 {
                return;
            }
        }
        profile.push(p);
    };

    for i in 0..params.teeth {
        let tc = i as f64 * pitch;

        // 1. Left root point (on root circle), then radial up to r_start.
        let left_root_ang = tc - half_tooth - inv_a + inv_start;
        push(polar(rd, left_root_ang));
        if r_start > rd {
            push(polar(r_start, left_root_ang));
        }

        // 2. Left flank: involute from r_start up to ra (leans toward center).
        for j in 0..=n_flank {
            let r = r_start + (ra - r_start) * j as f64 / n_flank as f64;
            let alpha_r = (rb / r).acos();
            let ang = tc - half_tooth - inv_a + inv(alpha_r);
            push(polar(r, ang));
        }

        // 3. Tip arc: left tip → right tip at radius ra.
        let left_tip = tc - half_tooth - inv_a + inv_a_tip;
        let right_tip = tc + half_tooth + inv_a - inv_a_tip;
        for j in 1..=n_tip {
            let ang = left_tip + (right_tip - left_tip) * j as f64 / n_tip as f64;
            push(polar(ra, ang));
        }

        // 4. Right flank: involute from ra down to r_start.
        for j in (0..=n_flank).rev() {
            let r = r_start + (ra - r_start) * j as f64 / n_flank as f64;
            let alpha_r = (rb / r).acos();
            let ang = tc + half_tooth + inv_a - inv(alpha_r);
            push(polar(r, ang));
        }

        // 5. Right root: radial down to rd.
        let right_root_ang = tc + half_tooth + inv_a - inv_start;
        if r_start > rd {
            push(polar(rd, right_root_ang));
        }

        // 6. Root arc: right root → next tooth's left root.
        let next_left_root = (tc + pitch) - half_tooth - inv_a + inv_start;
        for j in 1..n_root {
            let ang = right_root_ang + (next_left_root - right_root_ang) * j as f64 / n_root as f64;
            push(polar(rd, ang));
        }
    }

    let mut sketch = closed_loop_sketch(&profile);
    // Reference point at gear center (not part of the profile).
    sketch.add_point(0.0, 0.0);
    sketch
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radius(p: (f64, f64)) -> f64 {
        (p.0.powi(2) + p.1.powi(2)).sqrt()
    }

    #[test]
    fn gear_profile_radii_within_bounds() {
        let params = GearParams::default();
        let sketch = generate_gear_sketch(&params);
        let m = params.module;
        let z = params.teeth as f64;
        let rp = m * z / 2.0;
        let ra = rp + params.addendum_coef * m;
        let rd = rp - params.dedendum_coef * m;

        // Collect radii of all points (skip the center reference point).
        let mut radii = Vec::new();
        for e in sketch.entities.values() {
            if let echi_core::sketch::SketchEntity::Point(p) = e {
                let r = (p.x.powi(2) + p.y.powi(2)).sqrt();
                if r > 1e-6 {
                    radii.push(r);
                }
            }
        }
        assert!(radii.len() > 50);
        for r in &radii {
            assert!(*r >= rd - 1e-6, "point below root circle: {}", r);
            assert!(*r <= ra + 1e-6, "point above tip circle: {}", r);
        }
        // Must actually reach both tip and root.
        assert!(radii.iter().any(|r| (*r - ra).abs() < 1e-3), "no point on tip circle");
        assert!(radii.iter().any(|r| (*r - rd).abs() < 1e-3), "no point on root circle");
    }

    #[test]
    fn gear_loop_is_closed() {
        let params = GearParams::default();
        let sketch = generate_gear_sketch(&params);
        // Every profile point must have exactly 2 incident lines (closed ring).
        use std::collections::HashMap;
        let mut degree: HashMap<EntityId, usize> = HashMap::new();
        for e in sketch.entities.values() {
            if let echi_core::sketch::SketchEntity::Line { start, end, .. } = e {
                *degree.entry(*start).or_insert(0) += 1;
                *degree.entry(*end).or_insert(0) += 1;
            }
        }
        // The center reference point has degree 0; all profile points degree 2.
        let isolated = degree.iter().filter(|&(_, &d)| d == 0).count();
        assert!(isolated <= 1, "at most the center point may be isolated");
        for (_, &d) in degree.iter() {
            assert_eq!(d, 2, "profile point must have exactly 2 incident lines");
        }
    }

    #[test]
    fn tooth_thinner_at_tip_than_at_pitch() {
        // Verify the fundamental involute property: tooth angular width
        // decreases from root to tip.
        let m = 2.0;
        let z = 20.0f64;
        let alpha = 20f64.to_radians();
        let rp = m * z / 2.0;
        let ra = rp + m;
        let rb = rp * alpha.cos();
        let rd = rp - 1.25 * m;
        let r_start = rb.max(rd);
        let inv_a = inv(alpha);
        let half_tooth = PI / (2.0 * z);
        let inv = |x: f64| x.tan() - x;

        let width_at = |r: f64| -> f64 {
            let alpha_r = (rb / r).acos();
            let right = half_tooth + inv_a - inv(alpha_r);
            let left = -half_tooth - inv_a + inv(alpha_r);
            right - left
        };

        let w_root = width_at(r_start);
        let w_pitch = width_at(rp);
        let w_tip = width_at(ra);
        assert!(w_pitch > w_tip, "pitch width {} must exceed tip width {}", w_pitch, w_tip);
        assert!(w_root > w_pitch, "root width {} must exceed pitch width {}", w_root, w_pitch);
        // Sanity: pitch width ≈ half the angular pitch.
        assert!((w_pitch - PI / z).abs() < 1e-9);
        let _ = radius;
    }

    #[test]
    fn gear_extrudes_to_solid() {
        // The fixed gear must produce a valid closed loop that extrudes —
        // the previous duplicate-point closure made this return None.
        let params = GearParams::default();
        let sketch = generate_gear_sketch(&params);
        let mesh = echi_geom::extrude(
            &sketch,
            5.0,
            echi_core::feature::ExtrudeDirection::OneSide,
            0.0,
            &echi_core::feature::PlaneDefinition::XY,
        );
        assert!(mesh.is_some(), "gear profile must extrude into a solid");
        let mesh = mesh.unwrap();
        assert!(mesh.vertex_count() > 100);
        assert!(!mesh.positions.iter().any(|v| v.is_nan()));
    }
}
