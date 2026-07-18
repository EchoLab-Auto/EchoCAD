//! Cycloidal pin-wheel reducer （摆线针减速器） geometry generation.
//!
//! ## Geometry background
//!
//! A single-stage cycloidal reducer consists of:
//! - **Pin wheel （针轮）**: `z2` pins equally spaced on a circle of radius `R_p`
//! - **Cycloidal wheel （摆线轮）**: a disc with `z1 = z2 − 1` lobes, mounted on
//!   an eccentric of eccentricity `e`. The reduction ratio equals `z1`.
//!
//! ### Theoretical tooth profile (centerline through pin centers)
//!
//! Derived from the relative motion of a pin center in the wheel frame:
//!
//! ```text
//! x(t) = R_p·cos(t) − e·cos(z2·t)
//! y(t) = R_p·sin(t) − e·sin(z2·t)      t ∈ [0, 2π]
//! ```
//!
//! Its radius is `r(t) = √(R_p² + e² − 2·R_p·e·cos(z1·t))`, which oscillates
//! between `R_p − e` (valleys, at `t = 2πk/z1`) and `R_p + e` (lobe crests,
//! at `t = π(2k+1)/z1`) — exactly `z1` lobes, as required.
//!
//! ### Actual tooth surface
//!
//! The manufactured tooth surface is the centerline **offset inward by the
//! pin radius** `rp` (the pins wrap around the outside of the lobes):
//!
//! ```text
//! P_actual(t) = P_center(t) + rp · n_in(t)
//! ```
//!
//! where `n_in(t) = (−dy/dt, dx/dt) / |dP/dt|` is the unit normal pointing
//! toward the wheel center (verified at both valley and crest).

use echi_core::sketch::Sketch;
use std::f64::consts::PI;

/// Parameters for a cycloidal reducer stage.
#[derive(Debug, Clone)]
pub struct CycloidalParams {
    /// Number of cycloidal wheel teeth （摆线轮齿数）, = reduction ratio.
    pub z_cycloidal: u32,
    /// Pin center circle radius （针齿中心圆半径）, mm.
    pub pin_circle_radius: f64,
    /// Eccentricity （偏心距）, mm.
    pub eccentricity: f64,
    /// Pin radius （针齿半径）, mm.
    pub pin_radius: f64,
    /// Center bore diameter （中心孔径）, mm.
    pub bore_diameter: f64,
    /// Pin wheel outer diameter （针轮外径）, mm.
    pub ring_outer_diameter: f64,
    /// Curve sampling resolution (points per full revolution).
    pub curve_points: u32,
}

impl Default for CycloidalParams {
    fn default() -> Self {
        Self {
            z_cycloidal: 11,
            pin_circle_radius: 55.0,
            eccentricity: 2.0,
            pin_radius: 5.0,
            bore_diameter: 20.0,
            ring_outer_diameter: 140.0,
            curve_points: 360,
        }
    }
}

impl CycloidalParams {
    /// Number of pins （针齿数） = cycloidal teeth + 1.
    pub fn z_pins(&self) -> u32 {
        self.z_cycloidal + 1
    }

    /// Validate parameters; returns Err with a Chinese explanation on failure.
    pub fn validate(&self) -> Result<(), String> {
        let z1 = self.z_cycloidal as f64;
        let z2 = self.z_pins() as f64;
        let rp = self.pin_circle_radius;
        let e = self.eccentricity;
        let pr = self.pin_radius;

        if self.z_cycloidal < 5 {
            return Err("摆线轮齿数至少为 5".into());
        }
        if rp <= 0.0 || e <= 0.0 || pr <= 0.0 {
            return Err("半径与偏心距必须为正数".into());
        }
        // No-cusp condition: the epitrochoid must not loop back on itself.
        if e * z2 >= rp {
            return Err(format!(
                "偏心距过大：e·z2 ({:.2}) 必须小于针齿中心圆半径 ({:.2})",
                e * z2, rp
            ));
        }
        // Pin must fit inside the tooth space.
        if pr >= rp - e {
            return Err(format!(
                "针齿半径 ({:.2}) 必须小于 R_p − e ({:.2})",
                pr, rp - e
            ));
        }
        // Tooth height 2e must accommodate the pin diameter reasonably.
        if pr > 3.0 * e {
            return Err(format!(
                "针齿半径 ({:.2}) 相对齿高过大（建议 ≤ {:.2}）",
                pr, 3.0 * e
            ));
        }
        if self.bore_diameter <= 0.0 {
            return Err("中心孔径必须为正数".into());
        }
        if self.bore_diameter / 2.0 >= rp - e - pr {
            return Err("中心孔径过大，侵入齿根".into());
        }
        if self.ring_outer_diameter / 2.0 <= rp + pr {
            return Err(format!(
                "针轮外径 ({:.2}) 必须大于 2·(R_p + r_p) = {:.2}",
                self.ring_outer_diameter,
                2.0 * (rp + pr)
            ));
        }
        let _ = z1;
        Ok(())
    }
}

fn polar(r: f64, a: f64) -> (f64, f64) {
    (r * a.cos(), r * a.sin())
}

/// Theoretical cycloidal centerline point at parameter t.
fn centerline(rp: f64, e: f64, z2: f64, t: f64) -> (f64, f64) {
    (
        rp * t.cos() - e * (z2 * t).cos(),
        rp * t.sin() - e * (z2 * t).sin(),
    )
}

/// Generate the actual cycloidal wheel tooth profile (centerline offset
/// inward by pin radius), as a closed point loop.
pub fn cycloidal_tooth_profile(params: &CycloidalParams) -> Vec<(f64, f64)> {
    let rp = params.pin_circle_radius;
    let e = params.eccentricity;
    let pr = params.pin_radius;
    let z2 = params.z_pins() as f64;
    let n = params.curve_points.max(64) as usize;

    let mut profile = Vec::with_capacity(n);
    for i in 0..n {
        let t = 2.0 * PI * i as f64 / n as f64;
        let (x, y) = centerline(rp, e, z2, t);

        // Tangent dP/dt
        let dx = -rp * t.sin() + e * z2 * (z2 * t).sin();
        let dy = rp * t.cos() - e * z2 * (z2 * t).cos();
        let len = (dx * dx + dy * dy).sqrt().max(1e-12);

        // Inward unit normal (−dy, dx) — points toward wheel center.
        let nx = -dy / len;
        let ny = dx / len;

        profile.push((x + pr * nx, y + pr * ny));
    }
    profile
}

/// Tessellate a circle into a polygon loop (so multi-circle sketches work
/// with the extrude pipeline, which builds loops from line entities).
fn circle_loop(cx: f64, cy: f64, r: f64, n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let a = 2.0 * PI * i as f64 / n as f64;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// Append a closed loop of lines to the sketch from a point list.
fn push_loop(sketch: &mut Sketch, loop_pts: &[(f64, f64)]) {
    let n = loop_pts.len();
    if n < 3 {
        return;
    }
    let mut ids = Vec::with_capacity(n);
    for (x, y) in loop_pts {
        ids.push(sketch.add_point(*x, *y));
    }
    for i in 0..n {
        sketch.add_line(ids[i], ids[(i + 1) % n]);
    }
}

/// Generate the cycloidal wheel （摆线轮） sketch: tooth profile + center bore.
pub fn generate_cycloidal_wheel(params: &CycloidalParams) -> Sketch {
    let mut sketch = Sketch::new();

    // Outer tooth profile (actual surface, offset inward by pin radius).
    let tooth = cycloidal_tooth_profile(params);
    push_loop(&mut sketch, &tooth);

    // Center bore (hole). Tessellate so it forms a proper inner loop.
    let bore = circle_loop(0.0, 0.0, params.bore_diameter / 2.0, 48);
    push_loop(&mut sketch, &bore);

    // Reference center point.
    sketch.add_point(0.0, 0.0);
    sketch
}

/// Generate the pin wheel （针轮） sketch: outer ring + z2 pin holes.
pub fn generate_pin_wheel(params: &CycloidalParams) -> Sketch {
    let mut sketch = Sketch::new();
    let z2 = params.z_pins();
    let rp = params.pin_circle_radius;
    let pr = params.pin_radius;

    // Outer ring boundary.
    let ring = circle_loop(0.0, 0.0, params.ring_outer_diameter / 2.0, 96);
    push_loop(&mut sketch, &ring);

    // Pin holes, equally spaced on the pin circle. Pin 0 sits on +x axis.
    for k in 0..z2 {
        let a = 2.0 * PI * k as f64 / z2 as f64;
        let (cx, cy) = polar(rp, a);
        let hole = circle_loop(cx, cy, pr, 24);
        push_loop(&mut sketch, &hole);
    }

    // Reference center point.
    sketch.add_point(0.0, 0.0);
    sketch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rejects_bad_params() {
        let mut p = CycloidalParams::default();
        assert!(p.validate().is_ok());

        p.z_cycloidal = 3;
        assert!(p.validate().is_err());

        let mut p = CycloidalParams::default();
        p.eccentricity = 100.0;
        assert!(p.validate().is_err());

        let mut p = CycloidalParams::default();
        p.pin_radius = 100.0;
        assert!(p.validate().is_err());

        let mut p = CycloidalParams::default();
        p.ring_outer_diameter = 50.0;
        assert!(p.validate().is_err());
    }

    #[test]
    fn tooth_profile_has_z1_lobes() {
        let p = CycloidalParams::default(); // z1 = 11
        let profile = cycloidal_tooth_profile(&p);
        assert_eq!(profile.len(), p.curve_points as usize);

        // Count radius maxima — should equal z1.
        let radii: Vec<f64> = profile
            .iter()
            .map(|(x, y)| (x * x + y * y).sqrt())
            .collect();
        let n = radii.len();
        let mut maxima = 0;
        for i in 0..n {
            let prev = radii[(i + n - 1) % n];
            let next = radii[(i + 1) % n];
            if radii[i] >= prev && radii[i] > next {
                maxima += 1;
            }
        }
        assert_eq!(maxima, p.z_cycloidal as usize, "expected {} lobes", p.z_cycloidal);
    }

    #[test]
    fn tooth_surface_offset_inward() {
        let p = CycloidalParams::default();
        let rp = p.pin_circle_radius;
        let e = p.eccentricity;
        let pr = p.pin_radius;
        let profile = cycloidal_tooth_profile(&p);

        // At t=0 (valley), the centerline is at radius R_p − e; the actual
        // surface should be at R_p − e − r_p.
        let (x0, y0) = profile[0];
        let r0 = (x0 * x0 + y0 * y0).sqrt();
        let expect_valley = rp - e - pr;
        assert!(
            (r0 - expect_valley).abs() < 0.01,
            "valley radius {} should be ≈ {}",
            r0,
            expect_valley
        );

        // Max radius (crest) should be ≈ R_p + e − r_p.
        let max_r = profile
            .iter()
            .map(|(x, y)| (x * x + y * y).sqrt())
            .fold(0.0f64, f64::max);
        let expect_crest = rp + e - pr;
        assert!(
            (max_r - expect_crest).abs() < 0.05,
            "crest radius {} should be ≈ {}",
            max_r,
            expect_crest
        );
    }

    #[test]
    fn cycloidal_wheel_extrudes() {
        let p = CycloidalParams::default();
        let sketch = generate_cycloidal_wheel(&p);
        let mesh = echi_geom::extrude(
            &sketch,
            10.0,
            echi_core::feature::ExtrudeDirection::OneSide,
            0.0,
            &echi_core::feature::PlaneDefinition::XY,
        );
        assert!(mesh.is_some(), "cycloidal wheel must extrude");
        let mesh = mesh.unwrap();
        assert!(!mesh.positions.iter().any(|v| v.is_nan()));
    }

    #[test]
    fn pin_wheel_extrudes_with_holes() {
        let p = CycloidalParams::default();
        let sketch = generate_pin_wheel(&p);
        let mesh = echi_geom::extrude(
            &sketch,
            10.0,
            echi_core::feature::ExtrudeDirection::OneSide,
            0.0,
            &echi_core::feature::PlaneDefinition::XY,
        );
        assert!(mesh.is_some(), "pin wheel must extrude");
        let mesh = mesh.unwrap();
        assert!(!mesh.positions.iter().any(|v| v.is_nan()));
    }

    #[test]
    fn pin_wheel_has_correct_loop_count() {
        let p = CycloidalParams::default();
        let sketch = generate_pin_wheel(&p);
        // z2 pin holes + 1 outer ring, each a closed ring of degree-2 points.
        use std::collections::HashMap;
        let mut degree: HashMap<echi_core::sketch::EntityId, usize> = HashMap::new();
        for e in sketch.entities.values() {
            if let echi_core::sketch::SketchEntity::Line { start, end, .. } = e {
                *degree.entry(*start).or_insert(0) += 1;
                *degree.entry(*end).or_insert(0) += 1;
            }
        }
        // Every line endpoint participates in exactly 2 lines (closed rings).
        for (_, &d) in degree.iter() {
            assert_eq!(d, 2);
        }
        // Total loops = z2 + 1. Each loop is an independent ring; count rings
        // by counting connected components via the point adjacency.
        let total_lines = sketch
            .entities
            .values()
            .filter(|e| matches!(e, echi_core::sketch::SketchEntity::Line { .. }))
            .count();
        // 96 (ring) + z2 × 24 (holes)
        assert_eq!(total_lines, 96 + p.z_pins() as usize * 24);
    }
}
