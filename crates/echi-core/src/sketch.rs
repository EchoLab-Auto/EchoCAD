use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a sketch entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(pub u64);

/// A point in 2D sketch space.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SketchPoint {
    pub x: f64,
    pub y: f64,
}

impl SketchPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Geometric entity inside a sketch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SketchEntity {
    Point(SketchPoint),
    Line {
        start: EntityId,
        end: EntityId,
        #[serde(default)]
        construction: bool,
    },
    Circle {
        center: EntityId,
        radius: f64,
        #[serde(default)]
        construction: bool,
    },
    Arc {
        center: EntityId,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
        #[serde(default)]
        construction: bool,
    },
    /// Cubic spline defined by control points.
    Spline {
        control_points: Vec<EntityId>,
        #[serde(default)]
        construction: bool,
    },
    /// Ellipse defined by center, major axis endpoint, and minor/major ratio.
    Ellipse {
        center: EntityId,
        major_axis_end: EntityId,
        /// Ratio of minor radius to major radius (0 < ratio <= 1).
        ratio: f64,
        #[serde(default)]
        construction: bool,
    },
}

/// Constraint applied to sketch entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Constraint {
    /// Two points share the same location.
    Coincident { a: EntityId, b: EntityId },
    /// A line is horizontal.
    Horizontal { line: EntityId },
    /// A line is vertical.
    Vertical { line: EntityId },
    /// Fixed distance between two points.
    Distance { a: EntityId, b: EntityId, distance: f64 },
    /// Fixed radius of a circle.
    Radius { circle: EntityId, radius: f64 },
    /// Two lines are parallel.
    Parallel { line_a: EntityId, line_b: EntityId },
    /// Two lines are perpendicular.
    Perpendicular { line_a: EntityId, line_b: EntityId },
    /// A line is tangent to a circle.
    Tangent { line: EntityId, circle: EntityId },
    /// Two circles/arcs share the same center.
    Concentric { a: EntityId, b: EntityId },
    /// Two entities have equal length/radius.
    Equal { a: EntityId, b: EntityId },
    /// A point is fixed (anchored).
    Fix { point: EntityId },
    /// A point lies at the midpoint of a line.
    Midpoint { point: EntityId, line: EntityId },
    /// Two points are symmetric about a line.
    Symmetric { a: EntityId, b: EntityId, axis: EntityId },
    /// A point lies on a line.
    PointOnLine { point: EntityId, line: EntityId },
    /// Collinear: three points lie on the same line.
    Collinear { a: EntityId, b: EntityId, c: EntityId },
    /// Fixed angle between two lines (in degrees).
    Angle { line_a: EntityId, line_b: EntityId, angle_deg: f64 },
    /// Fixed diameter of a circle.
    Diameter { circle: EntityId, diameter: f64 },
}

/// A 2D sketch: a collection of entities and constraints.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sketch {
    next_id: u64,
    #[serde(with = "serde_entities")]
    pub entities: HashMap<EntityId, SketchEntity>,
    pub constraints: Vec<Constraint>,
}

/// Serde helper for `Sketch.entities`.  serde_json serializes `HashMap<u64, V>`
/// keys as strings (`"1"`, `"2"`, …), but `EntityId(u64)` is a newtype and
/// its derived `Deserialize` expects a numeric key.  This module bridges the
/// gap: serialise via `u64`, deserialise by collecting string keys and
/// parsing them back to `EntityId`.
mod serde_entities {
    use super::{EntityId, SketchEntity};
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};
    use std::collections::HashMap;
    use std::fmt;

    pub fn serialize<S: Serializer>(
        map: &HashMap<EntityId, SketchEntity>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(map.len()))?;
        for (k, v) in map {
            m.serialize_entry(&k.0, v)?;
        }
        m.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<HashMap<EntityId, SketchEntity>, D::Error> {
        struct EntitiesVisitor;
        impl<'de> Visitor<'de> for EntitiesVisitor {
            type Value = HashMap<EntityId, SketchEntity>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map with integer-string keys (serde_json serialized u64 keys as strings)")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut map = HashMap::with_capacity(access.size_hint().unwrap_or(0));
                while let Some(key_str) = access.next_key::<String>()? {
                    let id: u64 = key_str.parse().map_err(serde::de::Error::custom)?;
                    let val = access.next_value()?;
                    map.insert(EntityId(id), val);
                }
                Ok(map)
            }
        }
        d.deserialize_map(EntitiesVisitor)
    }
}

impl Sketch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_entity(&mut self, entity: SketchEntity) -> EntityId {
        let id = EntityId(self.next_id);
        self.next_id += 1;
        self.entities.insert(id, entity);
        id
    }

    pub fn add_constraint(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }

    pub fn get_point(&self, id: EntityId) -> Option<&SketchPoint> {
        match self.entities.get(&id) {
            Some(SketchEntity::Point(p)) => Some(p),
            _ => None,
        }
    }

    pub fn get_point_mut(&mut self, id: EntityId) -> Option<&mut SketchPoint> {
        match self.entities.get_mut(&id) {
            Some(SketchEntity::Point(p)) => Some(p),
            _ => None,
        }
    }

    pub fn add_point(&mut self, x: f64, y: f64) -> EntityId {
        self.add_entity(SketchEntity::Point(SketchPoint::new(x, y)))
    }

    pub fn add_line(&mut self, start: EntityId, end: EntityId) -> EntityId {
        self.add_entity(SketchEntity::Line { start, end, construction: false })
    }

    pub fn add_circle(&mut self, center: EntityId, radius: f64) -> EntityId {
        self.add_entity(SketchEntity::Circle { center, radius, construction: false })
    }

    pub fn add_arc(
        &mut self,
        center: EntityId,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    ) -> EntityId {
        self.add_entity(SketchEntity::Arc {
            center, radius, start_angle, end_angle, construction: false,
        })
    }

    pub fn add_spline(&mut self, control_points: Vec<EntityId>) -> EntityId {
        self.add_entity(SketchEntity::Spline { control_points, construction: false })
    }

    pub fn add_ellipse(
        &mut self,
        center: EntityId,
        major_axis_end: EntityId,
        ratio: f64,
    ) -> EntityId {
        self.add_entity(SketchEntity::Ellipse {
            center, major_axis_end, ratio, construction: false,
        })
    }

    /// Delete an entity. If it's a point referenced by other entities
    /// (line endpoints, circle/arc centers, spline control points, ellipse axes),
    /// those dependent entities are deleted too — otherwise they'd silently
    /// fall back to (0,0) at render time. Constraints referencing any of the
    /// deleted entities are dropped as well.
    ///
    /// Returns the IDs of all entities that were removed (including `id`).
    pub fn delete_entity_cascade(&mut self, id: EntityId) -> Vec<EntityId> {
        if !self.entities.contains_key(&id) {
            return Vec::new();
        }
        // Collect entities that reference the target.
        let mut to_delete: Vec<EntityId> = vec![id];
        let mut i = 0;
        while i < to_delete.len() {
            let target = to_delete[i];
            for (&eid, entity) in self.entities.iter() {
                if to_delete.contains(&eid) {
                    continue;
                }
                let references_target = match entity {
                    SketchEntity::Line { start, end, .. } => *start == target || *end == target,
                    SketchEntity::Circle { center, .. } | SketchEntity::Arc { center, .. } => *center == target,
                    SketchEntity::Ellipse { center, major_axis_end, .. } => {
                        *center == target || *major_axis_end == target
                    }
                    SketchEntity::Spline { control_points, .. } => control_points.contains(&target),
                    SketchEntity::Point(_) => false,
                };
                if references_target {
                    to_delete.push(eid);
                }
            }
            i += 1;
        }
        // Drop constraints that reference any of the deleted entities.
        let deleted: std::collections::HashSet<EntityId> = to_delete.iter().copied().collect();
        self.constraints.retain(|c| {
            use Constraint::*;
            match c {
                Coincident { a, b } => !deleted.contains(a) && !deleted.contains(b),
                Horizontal { line } | Vertical { line } => !deleted.contains(line),
                Distance { a, b, .. } => !deleted.contains(a) && !deleted.contains(b),
                Radius { circle, .. } | Diameter { circle, .. } => !deleted.contains(circle),
                Parallel { line_a, line_b } | Perpendicular { line_a, line_b } => {
                    !deleted.contains(line_a) && !deleted.contains(line_b)
                }
                Tangent { line, circle } => !deleted.contains(line) && !deleted.contains(circle),
                Concentric { a, b } | Equal { a, b } => !deleted.contains(a) && !deleted.contains(b),
                Fix { point } => !deleted.contains(point),
                Midpoint { point, line } => !deleted.contains(point) && !deleted.contains(line),
                Symmetric { a, b, axis } => {
                    !deleted.contains(a) && !deleted.contains(b) && !deleted.contains(axis)
                }
                PointOnLine { point, line } => !deleted.contains(point) && !deleted.contains(line),
                Collinear { a, b, c } => {
                    !deleted.contains(a) && !deleted.contains(b) && !deleted.contains(c)
                }
                Angle { line_a, line_b, .. } => !deleted.contains(line_a) && !deleted.contains(line_b),
            }
        });
        // Delete the entities themselves.
        for eid in &to_delete {
            self.entities.remove(eid);
        }
        to_delete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_center_point_cascades_to_circle() {
        let mut s = Sketch::new();
        let c = s.add_point(1.0, 2.0);
        let circle = s.add_circle(c, 0.5);
        let removed = s.delete_entity_cascade(c);
        assert!(removed.contains(&c));
        assert!(removed.contains(&circle));
        assert!(s.entities.is_empty());
    }

    #[test]
    fn delete_line_endpoint_cascades_and_drops_constraints() {
        let mut s = Sketch::new();
        let p1 = s.add_point(0.0, 0.0);
        let p2 = s.add_point(1.0, 0.0);
        let p3 = s.add_point(2.0, 0.0);
        let line1 = s.add_line(p1, p2);
        let _line2 = s.add_line(p2, p3);
        s.add_constraint(Constraint::Horizontal { line: line1 });

        let removed = s.delete_entity_cascade(p1);
        // p1 + line1 (references p1) should be gone; line2 should also go
        // because it shares the now-orphaned p2? No — p2 is still referenced
        // by line2 which is still alive, so p2 stays. Only line1 dies.
        assert!(removed.contains(&p1));
        assert!(removed.contains(&line1));
        assert!(!removed.contains(&p2));
        // Horizontal constraint on line1 should be dropped.
        assert!(s.constraints.is_empty());
        // p2, p3, line2 still alive.
        assert!(s.entities.contains_key(&p2));
        assert!(s.entities.contains_key(&p3));
    }

    #[test]
    fn delete_isolated_point_removes_nothing_else() {
        let mut s = Sketch::new();
        let p1 = s.add_point(0.0, 0.0);
        let p2 = s.add_point(5.0, 5.0);
        let removed = s.delete_entity_cascade(p1);
        assert_eq!(removed, vec![p1]);
        assert!(s.entities.contains_key(&p2));
    }

    #[test]
    fn delete_spline_control_point_cascades() {
        let mut s = Sketch::new();
        let p1 = s.add_point(0.0, 0.0);
        let p2 = s.add_point(1.0, 1.0);
        let p3 = s.add_point(2.0, 0.0);
        let spline = s.add_spline(vec![p1, p2, p3]);
        let removed = s.delete_entity_cascade(p2);
        assert!(removed.contains(&p2));
        assert!(removed.contains(&spline));
        // p1, p3 are still alive (not referenced by anything else).
        assert!(s.entities.contains_key(&p1));
        assert!(s.entities.contains_key(&p3));
    }
}
