use echi_core::sketch::{Constraint, EntityId, Sketch, SketchEntity, SketchPoint};
use nalgebra::{DMatrix, DVector};

/// Solve the sketch constraints in-place using Gauss-Newton.
///
/// Returns the number of iterations performed, or `None` if the sketch
/// is fully constrained (no free parameters).
pub fn solve(sketch: &mut Sketch, max_iters: usize, tolerance: f64) -> Option<usize> {
    let (point_ids, circle_ids) = extract_param_ids(sketch);

    if point_ids.is_empty() && circle_ids.is_empty() {
        return None;
    }

    let n = point_ids.len() * 2 + circle_ids.len();
    let mut params = DVector::zeros(n);
    write_params(sketch, &point_ids, &circle_ids, &mut params);

    for iter in 0..max_iters {
        let residuals = compute_residuals(sketch, &point_ids, &circle_ids, &params);
        let err = residuals.norm();
        if err < tolerance {
            read_params(sketch, &point_ids, &circle_ids, &params);
            return Some(iter);
        }

        let jacobian = compute_jacobian(sketch, &point_ids, &circle_ids, &params);

        // Gauss-Newton: (J^T J) dx = -J^T r
        let jt = jacobian.transpose();
        let jtj = &jt * &jacobian;
        let jtr = &jt * &residuals;

        // Add small Levenberg-Marquardt damping for stability.
        let damping = 1e-6;
        let a: DMatrix<f64> = jtj + damping * DMatrix::identity(n, n);

        match a.lu().solve(&(-jtr)) {
            Some(dx) => {
                params += dx;
            }
            None => {
                read_params(sketch, &point_ids, &circle_ids, &params);
                break;
            }
        }
    }

    read_params(sketch, &point_ids, &circle_ids, &params);
    Some(max_iters)
}

fn extract_param_ids(sketch: &Sketch) -> (Vec<EntityId>, Vec<EntityId>) {
    let mut point_ids = Vec::new();
    let mut circle_ids = Vec::new();

    // Collect fixed point IDs.
    let fixed_points: std::collections::HashSet<EntityId> = sketch
        .constraints
        .iter()
        .filter_map(|c| match c {
            Constraint::Fix { point } => Some(*point),
            _ => None,
        })
        .collect();

    for (&id, entity) in &sketch.entities {
        match entity {
            SketchEntity::Point(_) => {
                if !fixed_points.contains(&id) {
                    point_ids.push(id);
                }
            }
            SketchEntity::Circle { .. } => circle_ids.push(id),
            SketchEntity::Line { .. } | SketchEntity::Arc { .. } | SketchEntity::Spline { .. } | SketchEntity::Ellipse { .. } => {}
        }
    }
    (point_ids, circle_ids)
}

fn write_params(
    sketch: &Sketch,
    point_ids: &[EntityId],
    circle_ids: &[EntityId],
    params: &mut DVector<f64>,
) {
    for (i, id) in point_ids.iter().enumerate() {
        if let Some(p) = sketch.get_point(*id) {
            params[i * 2] = p.x;
            params[i * 2 + 1] = p.y;
        }
    }
    let offset = point_ids.len() * 2;
    for (i, id) in circle_ids.iter().enumerate() {
        if let Some(SketchEntity::Circle { radius, .. }) = sketch.entities.get(id) {
            params[offset + i] = *radius;
        }
    }
}

fn read_params(
    sketch: &mut Sketch,
    point_ids: &[EntityId],
    circle_ids: &[EntityId],
    params: &DVector<f64>,
) {
    for (i, id) in point_ids.iter().enumerate() {
        if let Some(p) = sketch.get_point_mut(*id) {
            p.x = params[i * 2];
            p.y = params[i * 2 + 1];
        }
    }
    let offset = point_ids.len() * 2;
    for (i, id) in circle_ids.iter().enumerate() {
        if let Some(SketchEntity::Circle { radius, .. }) = sketch.entities.get_mut(id) {
            *radius = params[offset + i];
        }
    }
}

fn get_point(params: &DVector<f64>, point_ids: &[EntityId], id: EntityId) -> Option<SketchPoint> {
    point_ids.iter().position(|pid| *pid == id).map(|idx| SketchPoint {
        x: params[idx * 2],
        y: params[idx * 2 + 1],
    })
}

fn get_radius(params: &DVector<f64>, point_ids: &[EntityId], circle_ids: &[EntityId], id: EntityId) -> Option<f64> {
    let offset = point_ids.len() * 2;
    circle_ids.iter().position(|pid| *pid == id).map(|idx| params[offset + idx])
}

fn compute_residuals(
    sketch: &Sketch,
    point_ids: &[EntityId],
    circle_ids: &[EntityId],
    params: &DVector<f64>,
) -> DVector<f64> {
    let mut residuals = Vec::new();
    for c in &sketch.constraints {
        match c {
            Constraint::Coincident { a, b } => {
                if let (Some(pa), Some(pb)) =
                    (get_point(params, point_ids, *a), get_point(params, point_ids, *b))
                {
                    residuals.push(pa.x - pb.x);
                    residuals.push(pa.y - pb.y);
                }
            }
            Constraint::Horizontal { line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if let (Some(ps), Some(pe)) =
                        (get_point(params, point_ids, *start), get_point(params, point_ids, *end))
                    {
                        residuals.push(pe.y - ps.y);
                    }
                }
            }
            Constraint::Vertical { line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if let (Some(ps), Some(pe)) =
                        (get_point(params, point_ids, *start), get_point(params, point_ids, *end))
                    {
                        residuals.push(pe.x - ps.x);
                    }
                }
            }
            Constraint::Distance { a, b, distance } => {
                if let (Some(pa), Some(pb)) =
                    (get_point(params, point_ids, *a), get_point(params, point_ids, *b))
                {
                    let dx = pa.x - pb.x;
                    let dy = pa.y - pb.y;
                    residuals.push((dx * dx + dy * dy).sqrt() - distance);
                }
            }
            Constraint::Radius { circle, radius } => {
                if let Some(r) = get_radius(params, point_ids, circle_ids, *circle) {
                    residuals.push(r - radius);
                }
            }
            // ── New constraints ──────────────────────────────────
            Constraint::Parallel { line_a, line_b } => {
                if let (
                    Some(SketchEntity::Line { start: s1, end: e1, .. }),
                    Some(SketchEntity::Line { start: s2, end: e2, .. }),
                ) = (sketch.entities.get(line_a), sketch.entities.get(line_b))
                {
                    if let (Some(p1s), Some(p1e), Some(p2s), Some(p2e)) = (
                        get_point(params, point_ids, *s1),
                        get_point(params, point_ids, *e1),
                        get_point(params, point_ids, *s2),
                        get_point(params, point_ids, *e2),
                    ) {
                        // Cross product of direction vectors should be zero
                        let dx1 = p1e.x - p1s.x;
                        let dy1 = p1e.y - p1s.y;
                        let dx2 = p2e.x - p2s.x;
                        let dy2 = p2e.y - p2s.y;
                        residuals.push(dx1 * dy2 - dy1 * dx2);
                    }
                }
            }
            Constraint::Perpendicular { line_a, line_b } => {
                if let (
                    Some(SketchEntity::Line { start: s1, end: e1, .. }),
                    Some(SketchEntity::Line { start: s2, end: e2, .. }),
                ) = (sketch.entities.get(line_a), sketch.entities.get(line_b))
                {
                    if let (Some(p1s), Some(p1e), Some(p2s), Some(p2e)) = (
                        get_point(params, point_ids, *s1),
                        get_point(params, point_ids, *e1),
                        get_point(params, point_ids, *s2),
                        get_point(params, point_ids, *e2),
                    ) {
                        // Dot product of direction vectors should be zero
                        let dx1 = p1e.x - p1s.x;
                        let dy1 = p1e.y - p1s.y;
                        let dx2 = p2e.x - p2s.x;
                        let dy2 = p2e.y - p2s.y;
                        residuals.push(dx1 * dx2 + dy1 * dy2);
                    }
                }
            }
            Constraint::Tangent { line, circle } => {
                if let (
                    Some(SketchEntity::Line { start, end, .. }),
                    Some(SketchEntity::Circle { center, .. }),
                ) = (sketch.entities.get(line), sketch.entities.get(circle))
                {
                    if let (Some(ps), Some(pe), Some(pc)) = (
                        get_point(params, point_ids, *start),
                        get_point(params, point_ids, *end),
                        get_point(params, point_ids, *center),
                    ) {
                        let r = get_radius(params, point_ids, circle_ids, *circle).unwrap_or(1.0);
                        // Distance from line to center = radius
                        let dx = pe.x - ps.x;
                        let dy = pe.y - ps.y;
                        let len = (dx * dx + dy * dy).sqrt();
                        if len > 1e-10 {
                            let dist = (dy * pc.x - dx * pc.y + pe.x * ps.y - pe.y * ps.x).abs()
                                / len;
                            residuals.push(dist - r.abs());
                        }
                    }
                }
            }
            Constraint::Concentric { a, b } => {
                if let (Some(pca), Some(pcb)) = (
                    get_circle_center(sketch, *a),
                    get_circle_center(sketch, *b),
                ) {
                    if let (Some(pa), Some(pb)) =
                        (get_point(params, point_ids, pca), get_point(params, point_ids, pcb))
                    {
                        residuals.push(pa.x - pb.x);
                        residuals.push(pa.y - pb.y);
                    }
                }
            }
            Constraint::Equal { a, b } => {
                match (sketch.entities.get(a), sketch.entities.get(b)) {
                    (
                        Some(SketchEntity::Line { start: s1, end: e1, .. }),
                        Some(SketchEntity::Line { start: s2, end: e2, .. }),
                    ) => {
                        if let (Some(p1s), Some(p1e), Some(p2s), Some(p2e)) = (
                            get_point(params, point_ids, *s1),
                            get_point(params, point_ids, *e1),
                            get_point(params, point_ids, *s2),
                            get_point(params, point_ids, *e2),
                        ) {
                            let l1 =
                                ((p1e.x - p1s.x).powi(2) + (p1e.y - p1s.y).powi(2)).sqrt();
                            let l2 =
                                ((p2e.x - p2s.x).powi(2) + (p2e.y - p2s.y).powi(2)).sqrt();
                            residuals.push(l1 - l2);
                        }
                    }
                    _ => {
                        if let (Some(r1), Some(r2)) = (
                            get_circle_radius(sketch, params, point_ids, circle_ids, *a),
                            get_circle_radius(sketch, params, point_ids, circle_ids, *b),
                        ) {
                            residuals.push(r1 - r2);
                        }
                    }
                }
            }
            Constraint::Fix { .. } => {
                // Fixed points are excluded from the parameter list entirely.
            }
            Constraint::Midpoint { point, line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if let (Some(pm), Some(ps), Some(pe)) = (
                        get_point(params, point_ids, *point),
                        get_point(params, point_ids, *start),
                        get_point(params, point_ids, *end),
                    ) {
                        residuals.push(pm.x - (ps.x + pe.x) / 2.0);
                        residuals.push(pm.y - (ps.y + pe.y) / 2.0);
                    }
                }
            }
            Constraint::Symmetric { a, b, axis } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(axis) {
                    if let (Some(pa), Some(pb), Some(p0), Some(p1)) = (
                        get_point(params, point_ids, *a),
                        get_point(params, point_ids, *b),
                        get_point(params, point_ids, *start),
                        get_point(params, point_ids, *end),
                    ) {
                        let dx = p1.x - p0.x;
                        let dy = p1.y - p0.y;
                        let len2 = dx * dx + dy * dy;
                        if len2 > 1e-10 {
                            // Project midpoint onto axis
                            let mx = (pa.x + pb.x) / 2.0;
                            let my = (pa.y + pb.y) / 2.0;
                            let t = ((mx - p0.x) * dx + (my - p0.y) * dy) / len2;
                            residuals.push(mx - (p0.x + t * dx));
                            residuals.push(my - (p0.y + t * dy));
                            // Distance from a and b to axis should be equal
                            let da = ((pa.y - p0.y) * dx - (pa.x - p0.x) * dy).abs() / len2.sqrt();
                            let db = ((pb.y - p0.y) * dx - (pb.x - p0.x) * dy).abs() / len2.sqrt();
                            residuals.push(da - db);
                        }
                    }
                }
            }
            Constraint::PointOnLine { point, line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if let (Some(pp), Some(ps), Some(pe)) = (
                        get_point(params, point_ids, *point),
                        get_point(params, point_ids, *start),
                        get_point(params, point_ids, *end),
                    ) {
                        let dx = pe.x - ps.x;
                        let dy = pe.y - ps.y;
                        // Cross product: (pp - ps) × direction should be zero
                        residuals.push((pp.x - ps.x) * dy - (pp.y - ps.y) * dx);
                    }
                }
            }
            Constraint::Collinear { a, b, c } => {
                if let (Some(pa), Some(pb), Some(pc)) = (
                    get_point(params, point_ids, *a),
                    get_point(params, point_ids, *b),
                    get_point(params, point_ids, *c),
                ) {
                    let area = (pa.x * (pb.y - pc.y)
                        + pb.x * (pc.y - pa.y)
                        + pc.x * (pa.y - pb.y))
                        .abs();
                    residuals.push(area);
                }
            }
            Constraint::Angle {
                line_a,
                line_b,
                angle_deg,
            } => {
                if let (
                    Some(SketchEntity::Line { start: s1, end: e1, .. }),
                    Some(SketchEntity::Line { start: s2, end: e2, .. }),
                ) = (sketch.entities.get(line_a), sketch.entities.get(line_b))
                {
                    if let (Some(p1s), Some(p1e), Some(p2s), Some(p2e)) = (
                        get_point(params, point_ids, *s1),
                        get_point(params, point_ids, *e1),
                        get_point(params, point_ids, *s2),
                        get_point(params, point_ids, *e2),
                    ) {
                        let dx1 = p1e.x - p1s.x;
                        let dy1 = p1e.y - p1s.y;
                        let dx2 = p2e.x - p2s.x;
                        let dy2 = p2e.y - p2s.y;
                        let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
                        let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();
                        if len1 > 1e-10 && len2 > 1e-10 {
                            let cos_a = (dx1 * dx2 + dy1 * dy2) / (len1 * len2);
                            let cos_a = cos_a.clamp(-1.0, 1.0);
                            let actual = cos_a.acos().to_degrees();
                            residuals.push(actual - angle_deg);
                        }
                    }
                }
            }
            Constraint::Diameter { circle, diameter } => {
                if let Some(r) = get_radius(params, point_ids, circle_ids, *circle) {
                    residuals.push(2.0 * r - diameter);
                }
            }
        }
    }
    DVector::from_vec(residuals)
}

/// Get the center point ID of a circle entity.
fn get_circle_center(sketch: &Sketch, id: EntityId) -> Option<EntityId> {
    match sketch.entities.get(&id) {
        Some(SketchEntity::Circle { center, .. }) | Some(SketchEntity::Arc { center, .. }) => {
            Some(*center)
        }
        _ => None,
    }
}

/// Get the radius of a circle/arc entity.
fn get_circle_radius(
    sketch: &Sketch,
    params: &DVector<f64>,
    point_ids: &[EntityId],
    circle_ids: &[EntityId],
    id: EntityId,
) -> Option<f64> {
    match sketch.entities.get(&id) {
        Some(SketchEntity::Circle { radius, .. }) | Some(SketchEntity::Arc { radius, .. }) => {
            Some(*radius)
        }
        _ => get_radius(params, point_ids, circle_ids, id),
    }
}

/// Compute Jacobian using finite differences (primary, handles all constraint types).
fn compute_jacobian(
    sketch: &Sketch,
    point_ids: &[EntityId],
    circle_ids: &[EntityId],
    params: &DVector<f64>,
) -> DMatrix<f64> {
    let eps = 1e-8;
    let r0 = compute_residuals(sketch, point_ids, circle_ids, params);
    let n = params.len();
    let m = r0.len();
    let mut j = DMatrix::zeros(m, n);
    for col in 0..n {
        let mut params_perturbed = params.clone();
        params_perturbed[col] += eps;
        let r_perturbed = compute_residuals(sketch, point_ids, circle_ids, &params_perturbed);
        for row in 0..m {
            j[(row, col)] = (r_perturbed[row] - r0[row]) / eps;
        }
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(2.0, 1.5);
        let line = sketch.add_line(a, b);
        sketch.add_constraint(Constraint::Horizontal { line });

        solve(&mut sketch, 50, 1e-6);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        assert!((pb.y - pa.y).abs() < 1e-4, "line should be horizontal");
    }

    #[test]
    fn distance_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(3.0, 4.0);
        sketch.add_constraint(Constraint::Distance { a, b, distance: 5.0 });

        solve(&mut sketch, 50, 1e-6);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        let d = ((pb.x - pa.x).powi(2) + (pb.y - pa.y).powi(2)).sqrt();
        assert!((d - 5.0).abs() < 1e-4, "distance should be 5.0");
    }
}
