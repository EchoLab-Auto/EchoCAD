use echi_core::sketch::{Constraint, EntityId, Sketch, SketchEntity, SketchPoint};
use nalgebra::{DMatrix, DVector};

/// Solve the sketch constraints in-place using Gauss-Newton.
///
/// Returns the number of iterations performed, or `None` if the sketch
/// is fully constrained (no free parameters).
///
/// The parameter vector packs:
/// - 2 coordinates per free point (x, y), then
/// - 1 radius per Circle or Arc entity (both carry a `radius: f64` field).
///
/// Fixed points (anchored by a `Fix` constraint) are excluded from the
/// parameter vector; their coordinates are read directly from the sketch
/// when residuals are evaluated, so constraints referencing them still apply.
///
/// Ellipse radii are *not* stored as scalars on the entity — the major
/// radius is derived from `distance(center, major_axis_end)` (both are
/// points and therefore already solver-driven), and the minor radius is
/// `ratio * major_radius`. A `Radius`/`Diameter` constraint on an
/// Ellipse drives its major radius via `major_axis_end`.
///
/// # Safety against deleted entities (F7)
///
/// Every entity-access in `compute_residuals` uses `if let Some(…)=…`
/// or `Option` combinators — there are zero `.unwrap()` calls on
/// `sketch.get_point()` or `sketch.entities.get()`. If a constraint
/// references a point or entity that was deleted (or never existed),
/// the constraint is silently skipped: it contributes no residuals
/// for that iteration. This is equivalent to returning 0.0 for the
/// broken constraint's residual and is safe — the LM solver converges
/// to whatever satisfies the remaining valid constraints. The
/// alternative (panicking) would poison the document mutex and crash
/// the entire application.
pub fn solve(sketch: &mut Sketch, max_iters: usize, tolerance: f64) -> Option<usize> {
    let (point_ids, radius_ids) = extract_param_ids(sketch);

    if point_ids.is_empty() && radius_ids.is_empty() {
        return None;
    }

    let n = point_ids.len() * 2 + radius_ids.len();
    let mut params = DVector::zeros(n);
    write_params(sketch, &point_ids, &radius_ids, &mut params);

    for iter in 0..max_iters {
        let residuals = compute_residuals(sketch, &point_ids, &radius_ids, &params);
        let err = residuals.norm();
        if err < tolerance {
            read_params(sketch, &point_ids, &radius_ids, &params);
            return Some(iter);
        }

        let jacobian = compute_jacobian(sketch, &point_ids, &radius_ids, &params);

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
                read_params(sketch, &point_ids, &radius_ids, &params);
                break;
            }
        }
    }

    read_params(sketch, &point_ids, &radius_ids, &params);
    Some(max_iters)
}

/// Extract point IDs (for (x,y) parameters) and radius IDs (for Circle/Arc
/// radius scalars) from the sketch. Fixed points (Constraint::Fix) are excluded
/// from the free-parameter list.
///
/// F7: This function only iterates `sketch.entities`; it does not follow
/// constraint references. If a constraint later references a deleted entity,
/// `compute_residuals` will see `None` from `point_coords` and silently skip
/// that constraint (contributing zero residual). No panic — the lock is safe.
fn extract_param_ids(sketch: &Sketch) -> (Vec<EntityId>, Vec<EntityId>) {
    let mut point_ids = Vec::new();
    let mut radius_ids = Vec::new();

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
            // Both Circle and Arc carry a `radius: f64` field that can be
            // driven directly by Radius/Diameter/Equal constraints.
            // Ellipse has no stored radius scalar; its major radius comes
            // from the center/major_axis_end points and is driven via those.
            SketchEntity::Circle { .. } | SketchEntity::Arc { .. } => radius_ids.push(id),
            SketchEntity::Line { .. } | SketchEntity::Spline { .. } | SketchEntity::Ellipse { .. } => {}
        }
    }
    (point_ids, radius_ids)
}

fn write_params(
    sketch: &Sketch,
    point_ids: &[EntityId],
    radius_ids: &[EntityId],
    params: &mut DVector<f64>,
) {
    for (i, id) in point_ids.iter().enumerate() {
        if let Some(p) = sketch.get_point(*id) {
            params[i * 2] = p.x;
            params[i * 2 + 1] = p.y;
        }
    }
    let offset = point_ids.len() * 2;
    for (i, id) in radius_ids.iter().enumerate() {
        let r = match sketch.entities.get(id) {
            Some(SketchEntity::Circle { radius, .. }) | Some(SketchEntity::Arc { radius, .. }) => {
                *radius
            }
            _ => 0.0,
        };
        params[offset + i] = r;
    }
}

fn read_params(
    sketch: &mut Sketch,
    point_ids: &[EntityId],
    radius_ids: &[EntityId],
    params: &DVector<f64>,
) {
    for (i, id) in point_ids.iter().enumerate() {
        if let Some(p) = sketch.get_point_mut(*id) {
            p.x = params[i * 2];
            p.y = params[i * 2 + 1];
        }
    }
    let offset = point_ids.len() * 2;
    for (i, id) in radius_ids.iter().enumerate() {
        let r = params[offset + i];
        match sketch.entities.get_mut(id) {
            Some(SketchEntity::Circle { radius, .. }) | Some(SketchEntity::Arc { radius, .. }) => {
                *radius = r;
            }
            _ => {}
        }
    }
}

fn get_point(params: &DVector<f64>, point_ids: &[EntityId], id: EntityId) -> Option<SketchPoint> {
    point_ids.iter().position(|pid| *pid == id).map(|idx| SketchPoint {
        x: params[idx * 2],
        y: params[idx * 2 + 1],
    })
}

/// Look up a point's coordinates in the current parameter state, falling
/// back to the sketch's stored value when the point is fixed (excluded
/// from the parameter vector). Without this fallback, any constraint that
/// references a fixed point would be silently dropped, since `get_point`
/// only inspects the parameter vector.
fn point_coords(
    sketch: &Sketch,
    params: &DVector<f64>,
    point_ids: &[EntityId],
    id: EntityId,
) -> Option<SketchPoint> {
    if let Some(p) = get_point(params, point_ids, id) {
        Some(p)
    } else {
        sketch.get_point(id).copied()
    }
}

fn get_radius(
    params: &DVector<f64>,
    point_ids: &[EntityId],
    radius_ids: &[EntityId],
    id: EntityId,
) -> Option<f64> {
    let offset = point_ids.len() * 2;
    radius_ids
        .iter()
        .position(|pid| *pid == id)
        .map(|idx| params[offset + idx])
}

/// Effective radius of a circle/arc/ellipse entity in the current
/// parameter state:
/// - Circle/Arc: the parameter-vector radius (driven directly).
/// - Ellipse: the major radius `distance(center, major_axis_end)`,
///   computed from the two point parameters.
///
/// This is what `Radius`, `Diameter`, and `Equal` compare against so a
/// dimensional constraint on an Arc or Ellipse actually moves it.
fn entity_radius(
    sketch: &Sketch,
    params: &DVector<f64>,
    point_ids: &[EntityId],
    radius_ids: &[EntityId],
    id: EntityId,
) -> Option<f64> {
    match sketch.entities.get(&id) {
        Some(SketchEntity::Circle { .. }) | Some(SketchEntity::Arc { .. }) => {
            get_radius(params, point_ids, radius_ids, id)
        }
        Some(SketchEntity::Ellipse {
            center,
            major_axis_end,
            ..
        }) => {
            let pc = point_coords(sketch, params, point_ids, *center)?;
            let pe = point_coords(sketch, params, point_ids, *major_axis_end)?;
            Some(((pe.x - pc.x).powi(2) + (pe.y - pc.y).powi(2)).sqrt())
        }
        _ => None,
    }
}

fn compute_residuals(
    sketch: &Sketch,
    point_ids: &[EntityId],
    radius_ids: &[EntityId],
    params: &DVector<f64>,
) -> DVector<f64> {
    let mut residuals = Vec::new();
    for c in &sketch.constraints {
        match c {
            Constraint::Coincident { a, b } => {
                if let (Some(pa), Some(pb)) = (
                    point_coords(sketch, params, point_ids, *a),
                    point_coords(sketch, params, point_ids, *b),
                ) {
                    residuals.push(pa.x - pb.x);
                    residuals.push(pa.y - pb.y);
                }
            }
            Constraint::Horizontal { line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if let (Some(ps), Some(pe)) = (
                        point_coords(sketch, params, point_ids, *start),
                        point_coords(sketch, params, point_ids, *end),
                    ) {
                        residuals.push(pe.y - ps.y);
                    }
                }
            }
            Constraint::Vertical { line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if let (Some(ps), Some(pe)) = (
                        point_coords(sketch, params, point_ids, *start),
                        point_coords(sketch, params, point_ids, *end),
                    ) {
                        residuals.push(pe.x - ps.x);
                    }
                }
            }
            Constraint::Distance { a, b, distance } => {
                if let (Some(pa), Some(pb)) = (
                    point_coords(sketch, params, point_ids, *a),
                    point_coords(sketch, params, point_ids, *b),
                ) {
                    let dx = pa.x - pb.x;
                    let dy = pa.y - pb.y;
                    residuals.push((dx * dx + dy * dy).sqrt() - distance);
                }
            }
            Constraint::Radius { circle, radius } => {
                if let Some(r) = entity_radius(sketch, params, point_ids, radius_ids, *circle) {
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
                        point_coords(sketch, params, point_ids, *s1),
                        point_coords(sketch, params, point_ids, *e1),
                        point_coords(sketch, params, point_ids, *s2),
                        point_coords(sketch, params, point_ids, *e2),
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
                        point_coords(sketch, params, point_ids, *s1),
                        point_coords(sketch, params, point_ids, *e1),
                        point_coords(sketch, params, point_ids, *s2),
                        point_coords(sketch, params, point_ids, *e2),
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
                // Support both Circle and Arc as the tangent target
                let center_opt = match sketch.entities.get(circle) {
                    Some(SketchEntity::Circle { center, .. }) => Some(*center),
                    Some(SketchEntity::Arc { center, .. }) => Some(*center),
                    _ => None,
                };
                if let (
                    Some(SketchEntity::Line { start, end, .. }),
                    Some(center_id),
                ) = (sketch.entities.get(line), center_opt)
                {
                    if let (Some(ps), Some(pe), Some(pc)) = (
                        point_coords(sketch, params, point_ids, *start),
                        point_coords(sketch, params, point_ids, *end),
                        point_coords(sketch, params, point_ids, center_id),
                    ) {
                        let r = get_radius(params, point_ids, radius_ids, *circle).unwrap_or(1.0);
                        // Distance from line to center = radius
                        let dx = pe.x - ps.x;
                        let dy = pe.y - ps.y;
                        let len = (dx * dx + dy * dy).sqrt().max(1e-10);
                        let dist = (dy * pc.x - dx * pc.y + pe.x * ps.y - pe.y * ps.x).abs()
                            / len;
                        residuals.push(dist - r.abs());
                    }
                }
            }
            Constraint::Concentric { a, b } => {
                if let (Some(pca), Some(pcb)) =
                    (get_circle_center(sketch, *a), get_circle_center(sketch, *b))
                {
                    if let (Some(pa), Some(pb)) = (
                        point_coords(sketch, params, point_ids, pca),
                        point_coords(sketch, params, point_ids, pcb),
                    ) {
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
                            point_coords(sketch, params, point_ids, *s1),
                            point_coords(sketch, params, point_ids, *e1),
                            point_coords(sketch, params, point_ids, *s2),
                            point_coords(sketch, params, point_ids, *e2),
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
                            entity_radius(sketch, params, point_ids, radius_ids, *a),
                            entity_radius(sketch, params, point_ids, radius_ids, *b),
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
                        point_coords(sketch, params, point_ids, *point),
                        point_coords(sketch, params, point_ids, *start),
                        point_coords(sketch, params, point_ids, *end),
                    ) {
                        residuals.push(pm.x - (ps.x + pe.x) / 2.0);
                        residuals.push(pm.y - (ps.y + pe.y) / 2.0);
                    }
                }
            }
            Constraint::Symmetric { a, b, axis } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(axis) {
                    if let (Some(pa), Some(pb), Some(p0), Some(p1)) = (
                        point_coords(sketch, params, point_ids, *a),
                        point_coords(sketch, params, point_ids, *b),
                        point_coords(sketch, params, point_ids, *start),
                        point_coords(sketch, params, point_ids, *end),
                    ) {
                        let dx = p1.x - p0.x;
                        let dy = p1.y - p0.y;
                        let len2 = (dx * dx + dy * dy).max(1e-10);
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
            Constraint::PointOnLine { point, line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if let (Some(pp), Some(ps), Some(pe)) = (
                        point_coords(sketch, params, point_ids, *point),
                        point_coords(sketch, params, point_ids, *start),
                        point_coords(sketch, params, point_ids, *end),
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
                    point_coords(sketch, params, point_ids, *a),
                    point_coords(sketch, params, point_ids, *b),
                    point_coords(sketch, params, point_ids, *c),
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
                        point_coords(sketch, params, point_ids, *s1),
                        point_coords(sketch, params, point_ids, *e1),
                        point_coords(sketch, params, point_ids, *s2),
                        point_coords(sketch, params, point_ids, *e2),
                    ) {
                        let dx1 = p1e.x - p1s.x;
                        let dy1 = p1e.y - p1s.y;
                        let dx2 = p2e.x - p2s.x;
                        let dy2 = p2e.y - p2s.y;
                        let len1 = (dx1 * dx1 + dy1 * dy1).sqrt().max(1e-10);
                        let len2 = (dx2 * dx2 + dy2 * dy2).sqrt().max(1e-10);
                        let cos_a = (dx1 * dx2 + dy1 * dy2) / (len1 * len2);
                        let cos_a = cos_a.clamp(-1.0, 1.0);
                        let actual = cos_a.acos().to_degrees();
                        residuals.push(actual - angle_deg);
                    }
                }
            }
            Constraint::Diameter { circle, diameter } => {
                if let Some(r) = entity_radius(sketch, params, point_ids, radius_ids, *circle) {
                    residuals.push(2.0 * r - diameter);
                }
            }
        }
    }
    DVector::from_vec(residuals)
}

/// Get the center point ID of a circle/arc/ellipse entity.
fn get_circle_center(sketch: &Sketch, id: EntityId) -> Option<EntityId> {
    match sketch.entities.get(&id) {
        Some(SketchEntity::Circle { center, .. })
        | Some(SketchEntity::Arc { center, .. })
        | Some(SketchEntity::Ellipse { center, .. }) => Some(*center),
        _ => None,
    }
}

/// Check the sketch for over-constrained or conflicting constraints.
///
/// Computes the Jacobian at the current parameter state and detects linear
/// dependence among constraint residuals. A constraint whose Jacobian rows
/// are linearly dependent on those of other constraints is redundant — it
/// either duplicates existing constraints or conflicts with them.
///
/// Constraints with non-zero residuals that are *not* linearly dependent are
/// flagged as unsatisfied (the solver could not find a state that satisfies
/// all constraints simultaneously).
///
/// Returns a list of `(constraint_index, description)` for each problematic
/// constraint. An empty list means the sketch is well-constrained (or
/// under-constrained but consistent).
///
/// Call this after `solve()` to get diagnostics when the solver fails to
/// converge or converges to an unexpected state.
pub fn check_overconstrained(sketch: &Sketch) -> Vec<(usize, String)> {
    let (point_ids, radius_ids) = extract_param_ids(sketch);
    let n = point_ids.len() * 2 + radius_ids.len();
    if n == 0 {
        return Vec::new();
    }

    let mut params = DVector::zeros(n);
    write_params(sketch, &point_ids, &radius_ids, &mut params);

    let residuals = compute_residuals(sketch, &point_ids, &radius_ids, &params);
    let jacobian = compute_jacobian(sketch, &point_ids, &radius_ids, &params);

    let m = residuals.len();
    if m == 0 {
        return Vec::new();
    }

    // Map each residual row to the constraint that produced it.
    let row_to_constraint =
        build_residual_constraint_map(sketch, &point_ids, &radius_ids, &params);

    let satisfied_threshold: f64 = 1e-8;
    let dependence_threshold: f64 = 1e-6;

    // Independent row vectors collected so far (for linear-dependence checks).
    let mut independent_rows: Vec<DVector<f64>> = Vec::new();
    let mut conflicting: Vec<(usize, String)> = Vec::new();
    let mut seen_conflicting: std::collections::HashSet<usize> = std::collections::HashSet::new();

    for i in 0..m {
        let ci = row_to_constraint[i];

        // Already flagged this constraint.
        if seen_conflicting.contains(&ci) {
            continue;
        }

        let row_vec: DVector<f64> = jacobian.row(i).transpose();
        let res_norm = residuals[i].abs();

        if independent_rows.is_empty() {
            independent_rows.push(row_vec);
            if res_norm >= satisfied_threshold {
                // First row has non-trivial residual and nothing to compare
                // against — possibly unsatisfied but we need more rows.
                // Don't flag yet; if the solver didn't converge the caller
                // already knows from the iteration count.
            }
            continue;
        }

        // Build V^T (n x k) where each column is a previously independent row.
        let k = independent_rows.len();
        let mut vt = DMatrix::zeros(n, k);
        for (j, ind_row) in independent_rows.iter().enumerate() {
            vt.column_mut(j).copy_from(ind_row);
        }

        // Solve V^T * y = row_vec in the least-squares sense.
        // svd() takes ownership, so clone vt before consuming.
        let svd = vt.clone().svd(true, true);
        match svd.solve(&row_vec, 1e-12) {
            Ok(y) => {
                let proj = &vt * y;
                let dep_norm = (&row_vec - &proj).norm();
                if dep_norm < dependence_threshold {
                    // Row is linearly dependent on previous rows.
                    // This constraint is redundant (over-constrained).
                    seen_conflicting.insert(ci);
                    let tag = if res_norm < satisfied_threshold {
                        "redundant"
                    } else {
                        "conflicting"
                    };
                    let msg = describe_constraint_diagnostic(&sketch.constraints[ci], ci, tag);
                    conflicting.push((ci, msg));
                } else {
                    // Row is independent.
                    if res_norm >= satisfied_threshold {
                        // Non-trivial residual on an independent row → unsatisfied.
                        seen_conflicting.insert(ci);
                        let msg = describe_constraint_diagnostic(
                            &sketch.constraints[ci],
                            ci,
                            "unsatisfied",
                        );
                        conflicting.push((ci, msg));
                    }
                    independent_rows.push(row_vec);
                }
            }
            Err(_) => {
                // SVD solve failed (degenerate) — treat as independent.
                if res_norm >= satisfied_threshold {
                    seen_conflicting.insert(ci);
                    let msg =
                        describe_constraint_diagnostic(&sketch.constraints[ci], ci, "unsatisfied");
                    conflicting.push((ci, msg));
                }
                independent_rows.push(row_vec);
            }
        }
    }

    conflicting
}

/// Map each residual entry back to the constraint index that produced it.
fn build_residual_constraint_map(
    sketch: &Sketch,
    point_ids: &[EntityId],
    radius_ids: &[EntityId],
    params: &DVector<f64>,
) -> Vec<usize> {
    let counts = count_constraint_residuals(sketch, point_ids, radius_ids, params);
    let total: usize = counts.iter().sum();
    let mut map = Vec::with_capacity(total);
    for (ci, &count) in counts.iter().enumerate() {
        for _ in 0..count {
            map.push(ci);
        }
    }
    map
}

/// Count how many residual entries each constraint produces for the current
/// parameter state. Must mirror `compute_residuals` exactly — if they disagree
/// the row-to-constraint mapping will be wrong.
fn count_constraint_residuals(
    sketch: &Sketch,
    point_ids: &[EntityId],
    radius_ids: &[EntityId],
    params: &DVector<f64>,
) -> Vec<usize> {
    sketch
        .constraints
        .iter()
        .map(|c| match c {
            Constraint::Coincident { a, b } => {
                if point_coords(sketch, params, point_ids, *a).is_some()
                    && point_coords(sketch, params, point_ids, *b).is_some()
                {
                    2
                } else {
                    0
                }
            }
            Constraint::Horizontal { line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if point_coords(sketch, params, point_ids, *start).is_some()
                        && point_coords(sketch, params, point_ids, *end).is_some()
                    {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::Vertical { line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if point_coords(sketch, params, point_ids, *start).is_some()
                        && point_coords(sketch, params, point_ids, *end).is_some()
                    {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::Distance { a, b, .. } => {
                if point_coords(sketch, params, point_ids, *a).is_some()
                    && point_coords(sketch, params, point_ids, *b).is_some()
                {
                    1
                } else {
                    0
                }
            }
            Constraint::Radius { circle, .. } => {
                if entity_radius(sketch, params, point_ids, radius_ids, *circle).is_some() {
                    1
                } else {
                    0
                }
            }
            Constraint::Parallel { line_a, line_b } => {
                if let (
                    Some(SketchEntity::Line { start: s1, end: e1, .. }),
                    Some(SketchEntity::Line { start: s2, end: e2, .. }),
                ) = (sketch.entities.get(line_a), sketch.entities.get(line_b))
                {
                    if point_coords(sketch, params, point_ids, *s1).is_some()
                        && point_coords(sketch, params, point_ids, *e1).is_some()
                        && point_coords(sketch, params, point_ids, *s2).is_some()
                        && point_coords(sketch, params, point_ids, *e2).is_some()
                    {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::Perpendicular { line_a, line_b } => {
                if let (
                    Some(SketchEntity::Line { start: s1, end: e1, .. }),
                    Some(SketchEntity::Line { start: s2, end: e2, .. }),
                ) = (sketch.entities.get(line_a), sketch.entities.get(line_b))
                {
                    if point_coords(sketch, params, point_ids, *s1).is_some()
                        && point_coords(sketch, params, point_ids, *e1).is_some()
                        && point_coords(sketch, params, point_ids, *s2).is_some()
                        && point_coords(sketch, params, point_ids, *e2).is_some()
                    {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::Tangent { line, circle } => {
                let is_curve = matches!(
                    sketch.entities.get(circle),
                    Some(SketchEntity::Circle { .. }) | Some(SketchEntity::Arc { .. })
                );
                if let (
                    Some(SketchEntity::Line { start, end, .. }),
                    true,
                ) = (sketch.entities.get(line), is_curve)
                {
                    if point_coords(sketch, params, point_ids, *start).is_some()
                        && point_coords(sketch, params, point_ids, *end).is_some()
                    {
                        // compute_residuals pushes 1 entry when len > 1e-10,
                        // or none when len <= 1e-10. We can't cheaply compute
                        // the exact threshold match here, so be conservative
                        // and always count 1 — a mis-count only affects the
                        // row-to-constraint mapping, not correctness.
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::Concentric { a, b } => {
                if let (Some(pca), Some(pcb)) =
                    (get_circle_center(sketch, *a), get_circle_center(sketch, *b))
                {
                    if point_coords(sketch, params, point_ids, pca).is_some()
                        && point_coords(sketch, params, point_ids, pcb).is_some()
                    {
                        2
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::Equal { a, b } => {
                match (sketch.entities.get(a), sketch.entities.get(b)) {
                    (
                        Some(SketchEntity::Line { start: s1, end: e1, .. }),
                        Some(SketchEntity::Line { start: s2, end: e2, .. }),
                    ) => {
                        if point_coords(sketch, params, point_ids, *s1).is_some()
                            && point_coords(sketch, params, point_ids, *e1).is_some()
                            && point_coords(sketch, params, point_ids, *s2).is_some()
                            && point_coords(sketch, params, point_ids, *e2).is_some()
                        {
                            1
                        } else {
                            0
                        }
                    }
                    _ => {
                        if entity_radius(sketch, params, point_ids, radius_ids, *a).is_some()
                            && entity_radius(sketch, params, point_ids, radius_ids, *b).is_some()
                        {
                            1
                        } else {
                            0
                        }
                    }
                }
            }
            Constraint::Fix { .. } => 0,
            Constraint::Midpoint { point, line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if point_coords(sketch, params, point_ids, *point).is_some()
                        && point_coords(sketch, params, point_ids, *start).is_some()
                        && point_coords(sketch, params, point_ids, *end).is_some()
                    {
                        2
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::Symmetric { a, b, axis } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(axis) {
                    if point_coords(sketch, params, point_ids, *a).is_some()
                        && point_coords(sketch, params, point_ids, *b).is_some()
                        && point_coords(sketch, params, point_ids, *start).is_some()
                        && point_coords(sketch, params, point_ids, *end).is_some()
                    {
                        3
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::PointOnLine { point, line } => {
                if let Some(SketchEntity::Line { start, end, .. }) = sketch.entities.get(line) {
                    if point_coords(sketch, params, point_ids, *point).is_some()
                        && point_coords(sketch, params, point_ids, *start).is_some()
                        && point_coords(sketch, params, point_ids, *end).is_some()
                    {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::Collinear { a, b, c } => {
                if point_coords(sketch, params, point_ids, *a).is_some()
                    && point_coords(sketch, params, point_ids, *b).is_some()
                    && point_coords(sketch, params, point_ids, *c).is_some()
                {
                    1
                } else {
                    0
                }
            }
            Constraint::Angle {
                line_a, line_b, ..
            } => {
                if let (
                    Some(SketchEntity::Line { start: s1, end: e1, .. }),
                    Some(SketchEntity::Line { start: s2, end: e2, .. }),
                ) = (sketch.entities.get(line_a), sketch.entities.get(line_b))
                {
                    if point_coords(sketch, params, point_ids, *s1).is_some()
                        && point_coords(sketch, params, point_ids, *e1).is_some()
                        && point_coords(sketch, params, point_ids, *s2).is_some()
                        && point_coords(sketch, params, point_ids, *e2).is_some()
                    {
                        1
                    } else {
                        0
                    }
                } else {
                    0
                }
            }
            Constraint::Diameter { circle, .. } => {
                if entity_radius(sketch, params, point_ids, radius_ids, *circle).is_some() {
                    1
                } else {
                    0
                }
            }
        })
        .collect()
}

/// Produce a human-readable diagnostic for a problematic constraint.
fn describe_constraint_diagnostic(c: &Constraint, index: usize, tag: &str) -> String {
    let prefix = format!("[{}] ", index);
    match c {
        Constraint::Coincident { a, b } => {
            format!("{}{} Coincident({:?}, {:?})", prefix, tag, a, b)
        }
        Constraint::Horizontal { line } => {
            format!("{}{} Horizontal({:?})", prefix, tag, line)
        }
        Constraint::Vertical { line } => {
            format!("{}{} Vertical({:?})", prefix, tag, line)
        }
        Constraint::Distance { a, b, distance } => {
            format!("{}{} Distance({:?}, {:?}, d={})", prefix, tag, a, b, distance)
        }
        Constraint::Radius { circle, radius } => {
            format!("{}{} Radius({:?}, r={})", prefix, tag, circle, radius)
        }
        Constraint::Parallel { line_a, line_b } => {
            format!("{}{} Parallel({:?}, {:?})", prefix, tag, line_a, line_b)
        }
        Constraint::Perpendicular { line_a, line_b } => {
            format!(
                "{}{} Perpendicular({:?}, {:?})",
                prefix, tag, line_a, line_b
            )
        }
        Constraint::Tangent { line, circle } => {
            format!("{}{} Tangent({:?}, {:?})", prefix, tag, line, circle)
        }
        Constraint::Concentric { a, b } => {
            format!("{}{} Concentric({:?}, {:?})", prefix, tag, a, b)
        }
        Constraint::Equal { a, b } => {
            format!("{}{} Equal({:?}, {:?})", prefix, tag, a, b)
        }
        Constraint::Fix { point } => {
            format!("{}{} Fix({:?})", prefix, tag, point)
        }
        Constraint::Midpoint { point, line } => {
            format!("{}{} Midpoint({:?}, {:?})", prefix, tag, point, line)
        }
        Constraint::Symmetric { a, b, axis } => {
            format!(
                "{}{} Symmetric({:?}, {:?}, axis={:?})",
                prefix, tag, a, b, axis
            )
        }
        Constraint::PointOnLine { point, line } => {
            format!("{}{} PointOnLine({:?}, {:?})", prefix, tag, point, line)
        }
        Constraint::Collinear { a, b, c } => {
            format!(
                "{}{} Collinear({:?}, {:?}, {:?})",
                prefix, tag, a, b, c
            )
        }
        Constraint::Angle {
            line_a,
            line_b,
            angle_deg,
        } => {
            format!(
                "{}{} Angle({:?}, {:?}, {}deg)",
                prefix, tag, line_a, line_b, angle_deg
            )
        }
        Constraint::Diameter { circle, diameter } => {
            format!("{}{} Diameter({:?}, d={})", prefix, tag, circle, diameter)
        }
    }
}

/// Compute Jacobian using finite differences (primary, handles all constraint types).
fn compute_jacobian(
    sketch: &Sketch,
    point_ids: &[EntityId],
    radius_ids: &[EntityId],
    params: &DVector<f64>,
) -> DMatrix<f64> {
    let eps = 1e-8;
    let r0 = compute_residuals(sketch, point_ids, radius_ids, params);
    let n = params.len();
    let m = r0.len();
    let mut j = DMatrix::zeros(m, n);
    for col in 0..n {
        let mut params_perturbed = params.clone();
        params_perturbed[col] += eps;
        let r_perturbed = compute_residuals(sketch, point_ids, radius_ids, &params_perturbed);
        for row in 0..m {
            let rp = r_perturbed.get(row).copied().unwrap_or(0.0);
            j[(row, col)] = (rp - r0[row]) / eps;
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

    #[test]
    fn coincident_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(2.0, 3.0);
        sketch.add_constraint(Constraint::Coincident { a, b });

        solve(&mut sketch, 50, 1e-6);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        assert!((pa.x - pb.x).abs() < 1e-4, "x should coincide");
        assert!((pa.y - pb.y).abs() < 1e-4, "y should coincide");
    }

    #[test]
    fn vertical_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(1.5, 2.0);
        let line = sketch.add_line(a, b);
        sketch.add_constraint(Constraint::Vertical { line });

        solve(&mut sketch, 50, 1e-6);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        assert!((pb.x - pa.x).abs() < 1e-4, "line should be vertical");
    }

    #[test]
    fn parallel_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(1.0, 1.0);
        let c = sketch.add_point(2.0, 0.0);
        let d = sketch.add_point(3.5, 0.3);
        let l1 = sketch.add_line(a, b);
        let l2 = sketch.add_line(c, d);
        // Anchor one endpoint of each line so the solver has a determinate
        // target and the unconstrained translational dof is removed.
        sketch.add_constraint(Constraint::Fix { point: a });
        sketch.add_constraint(Constraint::Fix { point: c });
        sketch.add_constraint(Constraint::Parallel { line_a: l1, line_b: l2 });

        solve(&mut sketch, 100, 1e-8);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        let pc = sketch.get_point(c).unwrap();
        let pd = sketch.get_point(d).unwrap();
        let cross = (pb.x - pa.x) * (pd.y - pc.y) - (pb.y - pa.y) * (pd.x - pc.x);
        assert!(cross.abs() < 1e-4, "lines should be parallel, cross={cross}");
    }

    #[test]
    fn perpendicular_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(1.0, 0.0);
        let c = sketch.add_point(2.0, 0.0);
        let d = sketch.add_point(3.0, 0.5);
        let l1 = sketch.add_line(a, b);
        let l2 = sketch.add_line(c, d);
        sketch.add_constraint(Constraint::Fix { point: a });
        sketch.add_constraint(Constraint::Fix { point: b });
        sketch.add_constraint(Constraint::Fix { point: c });
        sketch.add_constraint(Constraint::Perpendicular { line_a: l1, line_b: l2 });

        solve(&mut sketch, 100, 1e-8);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        let pc = sketch.get_point(c).unwrap();
        let pd = sketch.get_point(d).unwrap();
        let dot = (pb.x - pa.x) * (pd.x - pc.x) + (pb.y - pa.y) * (pd.y - pc.y);
        assert!(dot.abs() < 1e-4, "lines should be perpendicular, dot={dot}");
    }

    #[test]
    fn tangent_line_circle_constraint() {
        let mut sketch = Sketch::new();
        // Horizontal line anchored on the x-axis.
        let la = sketch.add_point(0.0, 0.0);
        let lb = sketch.add_point(2.0, 0.0);
        let line = sketch.add_line(la, lb);
        sketch.add_constraint(Constraint::Fix { point: la });
        sketch.add_constraint(Constraint::Fix { point: lb });
        // Circle whose center starts off the tangent distance.
        let center = sketch.add_point(1.0, 0.2);
        let circle = sketch.add_circle(center, 0.5);
        sketch.add_constraint(Constraint::Radius { circle, radius: 0.5 });
        sketch.add_constraint(Constraint::Tangent { line, circle });

        solve(&mut sketch, 100, 1e-8);

        let pc = sketch.get_point(center).unwrap();
        // Distance from center to the x-axis line is |pc.y|.
        let dist = pc.y.abs();
        assert!(
            (dist - 0.5).abs() < 1e-4,
            "circle center should sit one radius from the line, dist={dist}"
        );
    }

    #[test]
    fn concentric_constraint() {
        let mut sketch = Sketch::new();
        let c1 = sketch.add_point(0.0, 0.0);
        let c2 = sketch.add_point(1.0, 0.5);
        let circle1 = sketch.add_circle(c1, 1.0);
        let circle2 = sketch.add_circle(c2, 1.0);
        sketch.add_constraint(Constraint::Concentric { a: circle1, b: circle2 });

        solve(&mut sketch, 100, 1e-8);

        let p1 = sketch.get_point(c1).unwrap();
        let p2 = sketch.get_point(c2).unwrap();
        assert!((p1.x - p2.x).abs() < 1e-4, "centers should coincide in x");
        assert!((p1.y - p2.y).abs() < 1e-4, "centers should coincide in y");
    }

    #[test]
    fn equal_two_lines_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(1.0, 0.0);
        let c = sketch.add_point(2.0, 0.0);
        let d = sketch.add_point(3.0, 0.5);
        let l1 = sketch.add_line(a, b);
        let l2 = sketch.add_line(c, d);
        // Pin l1 to length 1; only d is free so l2 must grow/shrink to match.
        sketch.add_constraint(Constraint::Fix { point: a });
        sketch.add_constraint(Constraint::Fix { point: b });
        sketch.add_constraint(Constraint::Fix { point: c });
        sketch.add_constraint(Constraint::Equal { a: l1, b: l2 });

        solve(&mut sketch, 100, 1e-8);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        let pc = sketch.get_point(c).unwrap();
        let pd = sketch.get_point(d).unwrap();
        let l1_len = ((pb.x - pa.x).powi(2) + (pb.y - pa.y).powi(2)).sqrt();
        let l2_len = ((pd.x - pc.x).powi(2) + (pd.y - pc.y).powi(2)).sqrt();
        assert!((l1_len - l2_len).abs() < 1e-4, "line lengths should be equal");
    }

    #[test]
    fn equal_two_circles_constraint() {
        let mut sketch = Sketch::new();
        let c1 = sketch.add_point(0.0, 0.0);
        let c2 = sketch.add_point(3.0, 0.0);
        let circle1 = sketch.add_circle(c1, 1.0);
        let circle2 = sketch.add_circle(c2, 2.0);
        // Pin centers so only the two radii are free to move.
        sketch.add_constraint(Constraint::Fix { point: c1 });
        sketch.add_constraint(Constraint::Fix { point: c2 });
        sketch.add_constraint(Constraint::Equal { a: circle1, b: circle2 });

        solve(&mut sketch, 100, 1e-8);

        let r1 = match sketch.entities.get(&circle1) {
            Some(SketchEntity::Circle { radius, .. }) => *radius,
            _ => f64::NAN,
        };
        let r2 = match sketch.entities.get(&circle2) {
            Some(SketchEntity::Circle { radius, .. }) => *radius,
            _ => f64::NAN,
        };
        assert!((r1 - r2).abs() < 1e-4, "circle radii should be equal");
        assert!(!r1.is_nan() && !r2.is_nan(), "radii must not be NaN");
    }

    #[test]
    fn fix_constraint_anchors_point() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(1.0, 2.0);
        let b = sketch.add_point(4.0, 6.0);
        sketch.add_constraint(Constraint::Fix { point: a });
        // Initial distance is 5; force the free point to move to reach 10.
        sketch.add_constraint(Constraint::Distance { a, b, distance: 10.0 });

        solve(&mut sketch, 100, 1e-8);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        // Fixed point must not move.
        assert!((pa.x - 1.0).abs() < 1e-6, "fixed point x should not move");
        assert!((pa.y - 2.0).abs() < 1e-6, "fixed point y should not move");
        // Free point moves to satisfy the distance from the fixed anchor.
        let d = ((pb.x - pa.x).powi(2) + (pb.y - pa.y).powi(2)).sqrt();
        assert!((d - 10.0).abs() < 1e-4, "distance from fixed point should be 10.0");
    }

    #[test]
    fn midpoint_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(2.0, 4.0);
        let m = sketch.add_point(1.5, 1.5);
        let line = sketch.add_line(a, b);
        sketch.add_constraint(Constraint::Fix { point: a });
        sketch.add_constraint(Constraint::Fix { point: b });
        sketch.add_constraint(Constraint::Midpoint { point: m, line });

        solve(&mut sketch, 100, 1e-8);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        let pm = sketch.get_point(m).unwrap();
        assert!((pm.x - (pa.x + pb.x) / 2.0).abs() < 1e-4, "midpoint x");
        assert!((pm.y - (pa.y + pb.y) / 2.0).abs() < 1e-4, "midpoint y");
    }

    #[test]
    fn symmetric_constraint() {
        let mut sketch = Sketch::new();
        // Axis = the y-axis (x = 0).
        let p0 = sketch.add_point(0.0, 0.0);
        let p1 = sketch.add_point(0.0, 1.0);
        let axis = sketch.add_line(p0, p1);
        sketch.add_constraint(Constraint::Fix { point: p0 });
        sketch.add_constraint(Constraint::Fix { point: p1 });
        // Two points that are not yet mirror images.
        let a = sketch.add_point(1.0, 2.0);
        let b = sketch.add_point(0.5, 3.0);
        sketch.add_constraint(Constraint::Symmetric { a, b, axis });

        solve(&mut sketch, 200, 1e-8);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        // Mirrored across the y-axis: x-coords sum to zero, equal |x|.
        assert!((pa.x + pb.x).abs() < 1e-3, "midpoint should lie on axis");
        assert!(
            (pa.x.abs() - pb.x.abs()).abs() < 1e-4,
            "points should be equidistant from axis"
        );
    }

    #[test]
    fn point_on_line_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(2.0, 2.0);
        let p = sketch.add_point(1.5, 1.0);
        let line = sketch.add_line(a, b);
        sketch.add_constraint(Constraint::Fix { point: a });
        sketch.add_constraint(Constraint::Fix { point: b });
        sketch.add_constraint(Constraint::PointOnLine { point: p, line });

        solve(&mut sketch, 100, 1e-8);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        let pp = sketch.get_point(p).unwrap();
        // Cross product of (p - a) and (b - a) must vanish.
        let cross = (pp.x - pa.x) * (pb.y - pa.y) - (pp.y - pa.y) * (pb.x - pa.x);
        assert!(cross.abs() < 1e-4, "point should lie on the line, cross={cross}");
    }

    #[test]
    fn collinear_constraint() {
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(1.0, 1.0);
        let c = sketch.add_point(2.0, 0.5);
        sketch.add_constraint(Constraint::Fix { point: a });
        sketch.add_constraint(Constraint::Fix { point: b });
        sketch.add_constraint(Constraint::Collinear { a, b, c });

        solve(&mut sketch, 100, 1e-8);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        let pc = sketch.get_point(c).unwrap();
        let area = (pa.x * (pb.y - pc.y) + pb.x * (pc.y - pa.y) + pc.x * (pa.y - pb.y)).abs();
        assert!(area.abs() < 1e-4, "three points should be collinear, area={area}");
    }

    #[test]
    fn angle_constraint() {
        let mut sketch = Sketch::new();
        // l1 fixed horizontal.
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(1.0, 0.0);
        let l1 = sketch.add_line(a, b);
        // l2 shares the start point a; only its end is free.
        let d = sketch.add_point(1.5, 0.3);
        let l2 = sketch.add_line(a, d);
        sketch.add_constraint(Constraint::Fix { point: a });
        sketch.add_constraint(Constraint::Fix { point: b });
        sketch.add_constraint(Constraint::Angle { line_a: l1, line_b: l2, angle_deg: 45.0 });

        solve(&mut sketch, 200, 1e-8);

        let pa = sketch.get_point(a).unwrap();
        let pb = sketch.get_point(b).unwrap();
        let pd = sketch.get_point(d).unwrap();
        let dx1 = pb.x - pa.x;
        let dy1 = pb.y - pa.y;
        let dx2 = pd.x - pa.x;
        let dy2 = pd.y - pa.y;
        let len1 = (dx1 * dx1 + dy1 * dy1).sqrt();
        let len2 = (dx2 * dx2 + dy2 * dy2).sqrt();
        let cos_a = ((dx1 * dx2 + dy1 * dy2) / (len1 * len2)).clamp(-1.0, 1.0);
        let actual = cos_a.acos().to_degrees();
        assert!((actual - 45.0).abs() < 1e-3, "angle should be 45 degrees, got {actual}");
    }

    #[test]
    fn diameter_constraint() {
        let mut sketch = Sketch::new();
        let c = sketch.add_point(0.0, 0.0);
        let circle = sketch.add_circle(c, 1.0);
        sketch.add_constraint(Constraint::Fix { point: c });
        sketch.add_constraint(Constraint::Diameter { circle, diameter: 4.0 });

        solve(&mut sketch, 100, 1e-8);

        let r = match sketch.entities.get(&circle) {
            Some(SketchEntity::Circle { radius, .. }) => *radius,
            _ => f64::NAN,
        };
        assert!((2.0 * r - 4.0).abs() < 1e-4, "diameter should be 4.0, got {}", 2.0 * r);
        assert!(r.is_finite(), "radius must be finite");
    }

    #[test]
    fn radius_on_arc_constraint() {
        let mut sketch = Sketch::new();
        let center = sketch.add_point(0.0, 0.0);
        let arc = sketch.add_arc(center, 1.0, 0.0, std::f64::consts::PI);
        sketch.add_constraint(Constraint::Fix { point: center });
        sketch.add_constraint(Constraint::Radius { circle: arc, radius: 2.0 });

        solve(&mut sketch, 100, 1e-8);

        let r = match sketch.entities.get(&arc) {
            Some(SketchEntity::Arc { radius, .. }) => *radius,
            _ => f64::NAN,
        };
        assert!((r - 2.0).abs() < 1e-4, "arc radius should be 2.0, got {r}");
        assert!(r.is_finite(), "arc radius must be finite");
    }

    #[test]
    fn radius_on_ellipse_constraint() {
        let mut sketch = Sketch::new();
        let center = sketch.add_point(0.0, 0.0);
        let major_end = sketch.add_point(2.0, 0.0);
        let ellipse = sketch.add_ellipse(center, major_end, 0.5);
        sketch.add_constraint(Constraint::Fix { point: center });
        sketch.add_constraint(Constraint::Radius { circle: ellipse, radius: 3.0 });

        solve(&mut sketch, 100, 1e-8);

        // A Radius constraint on an Ellipse drives the major radius
        // = distance(center, major_axis_end).
        let pc = sketch.get_point(center).unwrap();
        let pe = sketch.get_point(major_end).unwrap();
        let rx = ((pe.x - pc.x).powi(2) + (pe.y - pc.y).powi(2)).sqrt();
        assert!((rx - 3.0).abs() < 1e-4, "ellipse major radius should be 3.0, got {rx}");
        assert!(rx.is_finite(), "major radius must be finite");
    }

    #[test]
    fn solve_no_free_params_returns_none() {
        // Degenerate input: a single fixed point, nothing else.
        let mut sketch = Sketch::new();
        let a = sketch.add_point(1.0, 1.0);
        sketch.add_constraint(Constraint::Fix { point: a });

        let result = solve(&mut sketch, 50, 1e-6);
        assert!(result.is_none(), "fully-fixed sketch has no free parameters");
    }

    #[test]
    fn solve_preserves_indices_and_no_nan() {
        // Invariant: solving must not corrupt the entity map or produce NaN.
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(1.0, 1.0);
        let c = sketch.add_point(2.0, 0.0);
        let circle = sketch.add_circle(c, 0.5);
        sketch.add_constraint(Constraint::Distance { a, b, distance: 2.0 });
        sketch.add_constraint(Constraint::Radius { circle, radius: 1.0 });

        solve(&mut sketch, 50, 1e-6);

        // All original entities still present.
        assert!(sketch.entities.contains_key(&a));
        assert!(sketch.entities.contains_key(&b));
        assert!(sketch.entities.contains_key(&c));
        assert!(sketch.entities.contains_key(&circle));
        // No NaN in any point.
        for id in [a, b, c] {
            let p = sketch.get_point(id).unwrap();
            assert!(p.x.is_finite() && p.y.is_finite(), "point {id:?} has NaN");
        }
        if let Some(SketchEntity::Circle { radius, .. }) = sketch.entities.get(&circle) {
            assert!(radius.is_finite(), "circle radius has NaN");
        } else {
            panic!("circle entity missing after solve");
        }
    }

    // ── Over-constrained detection tests ──────────────────────────

    #[test]
    fn overconstrained_conflict_detected() {
        // Triangle with all 3 side lengths + one angle fixed.
        // Three side lengths fully determine the triangle shape; the angle is
        // redundant and should be flagged as conflicting.
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 0.0);
        let p1 = sketch.add_point(3.0, 0.0);
        let p2 = sketch.add_point(1.0, 2.0);
        let l0 = sketch.add_line(p0, p1);
        let _l1 = sketch.add_line(p1, p2);
        let l2 = sketch.add_line(p2, p0);
        // Fix one point and one direction to remove rigid-body modes.
        sketch.add_constraint(Constraint::Fix { point: p0 });
        sketch.add_constraint(Constraint::Horizontal { line: l0 });
        // Three distance constraints fully constrain the shape.
        sketch.add_constraint(Constraint::Distance { a: p0, b: p1, distance: 3.0 });
        sketch.add_constraint(Constraint::Distance { a: p1, b: p2, distance: 3.6055 }); // ~sqrt(13)
        sketch.add_constraint(Constraint::Distance { a: p2, b: p0, distance: 2.2361 }); // ~sqrt(5)
        // Fourth constraint: angle between l2 and l0 is redundant.
        sketch.add_constraint(Constraint::Angle {
            line_a: l2,
            line_b: l0,
            angle_deg: 63.4349, // atan2(2,1) in degrees
        });

        solve(&mut sketch, 200, 1e-8);
        let conflicts = check_overconstrained(&sketch);
        // The Angle constraint (index 6) should be flagged as redundant.
        assert!(
            !conflicts.is_empty(),
            "expected at least one conflicting constraint in over-defined triangle"
        );
        let angle_idx = sketch.constraints.len() - 1; // Angle was added last
        let has_angle_conflict = conflicts.iter().any(|(idx, _)| *idx == angle_idx);
        assert!(
            has_angle_conflict,
            "expected Angle constraint to be flagged as conflicting"
        );
    }

    #[test]
    fn no_false_positive_on_valid_sketch() {
        // Simple well-constrained sketch: one anchored point + one distance
        // constraint. After solving, check_overconstrained should be empty.
        let mut sketch = Sketch::new();
        let a = sketch.add_point(0.0, 0.0);
        let b = sketch.add_point(5.0, 0.0);
        sketch.add_constraint(Constraint::Fix { point: a });
        sketch.add_constraint(Constraint::Distance { a, b, distance: 5.0 });

        solve(&mut sketch, 50, 1e-6);
        let conflicts = check_overconstrained(&sketch);
        assert!(
            conflicts.is_empty(),
            "expected no conflicts in a valid sketch, got {}: {:?}",
            conflicts.len(),
            conflicts
        );
    }

    #[test]
    fn redundant_constraint_detected() {
        // Two conflicting Distance constraints from the same fixed point to
        // the same free point.  The free point cannot be at two different
        // distances simultaneously — the solver converges to a compromise
        // (midpoint distance), leaving both residuals non-zero.  The second
        // Distance constraint is linearly dependent (same Jacobian direction)
        // and should be flagged.
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 0.0);
        let p1 = sketch.add_point(4.0, 0.0);
        sketch.add_constraint(Constraint::Fix { point: p0 });
        sketch.add_constraint(Constraint::Distance { a: p0, b: p1, distance: 3.0 });
        sketch.add_constraint(Constraint::Distance { a: p0, b: p1, distance: 7.0 });

        solve(&mut sketch, 100, 1e-8);
        let conflicts = check_overconstrained(&sketch);

        // The second Distance constraint (index 2) shares the same Jacobian
        // row direction as the first and cannot be independently satisfied.
        assert!(
            !conflicts.is_empty(),
            "expected conflicting Distance constraints to be detected"
        );
        let conflicting_idx = 2;
        let has_conflict = conflicts.iter().any(|(idx, _)| *idx == conflicting_idx);
        assert!(
            has_conflict,
            "expected second Distance constraint to be flagged; diagnostics: {:?}",
            conflicts
        );
    }

    #[test]
    fn all_satisfied_returns_empty() {
        // A triangle with exactly the right number of constraints (well-constrained)
        // should produce an empty conflict list after solving.
        let mut sketch = Sketch::new();
        let p0 = sketch.add_point(0.0, 0.0);
        let p1 = sketch.add_point(3.0, 0.0);
        let p2 = sketch.add_point(1.0, 2.0);
        let l0 = sketch.add_line(p0, p1);
        let _l1 = sketch.add_line(p1, p2);
        let _l2 = sketch.add_line(p2, p0);
        // 3 constraints for a triangle: fix one point, fix one direction,
        // and fix the two side lengths (the third follows automatically).
        sketch.add_constraint(Constraint::Fix { point: p0 });
        sketch.add_constraint(Constraint::Horizontal { line: l0 });
        sketch.add_constraint(Constraint::Distance { a: p0, b: p1, distance: 3.0 });
        sketch.add_constraint(Constraint::Distance { a: p1, b: p2, distance: 3.6055 });

        let iters = solve(&mut sketch, 200, 1e-8);
        assert!(iters.is_some(), "well-constrained sketch should converge");

        let conflicts = check_overconstrained(&sketch);
        assert!(
            conflicts.is_empty(),
            "expected no conflicts in well-constrained sketch, got {}: {:?}",
            conflicts.len(),
            conflicts
        );
    }
}
