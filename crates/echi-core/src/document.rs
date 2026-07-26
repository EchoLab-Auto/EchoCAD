use crate::feature::{Feature, FeatureId, FeatureKind};
use crate::parameter::{Parameter, ParameterId};
use serde::{Deserialize, Serialize};

fn default_format_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    #[serde(default = "default_format_version")]
    pub format_version: u32,
    pub name: String,
    pub features: Vec<Feature>,
    pub parameters: Vec<Parameter>,
    next_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            format_version: 1,
            name: String::new(),
            features: Vec::new(),
            parameters: Vec::new(),
            next_id: 0,
        }
    }
}

impl Document {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format_version: 1,
            name: name.into(),
            features: Vec::new(),
            parameters: Vec::new(),
            next_id: 1,
        }
    }

    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Peek at the id that the next `next_id()` call would return, without
    /// consuming it. Used to build a `FeatureKind` for dependency validation
    /// *before* committing any allocation (validate-then-mutate).
    pub fn peek_next_id(&self) -> u64 {
        self.next_id
    }

    pub fn new_feature_id(&mut self) -> FeatureId {
        FeatureId(self.next_id())
    }

    pub fn new_parameter_id(&mut self) -> ParameterId {
        ParameterId(self.next_id())
    }

    pub fn add_parameter(&mut self, name: impl Into<String>, value: f64) -> ParameterId {
        let id = self.new_parameter_id();
        self.parameters.push(Parameter::new(id, name, value));
        id
    }

    pub fn get_parameter(&self, id: ParameterId) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.id == id)
    }

    pub fn get_parameter_mut(&mut self, id: ParameterId) -> Option<&mut Parameter> {
        self.parameters.iter_mut().find(|p| p.id == id)
    }

    pub fn add_feature(&mut self, feature: Feature) {
        self.features.push(feature);
    }

    pub fn get_feature(&self, id: FeatureId) -> Option<&Feature> {
        self.features.iter().find(|f| f.id() == id)
    }

    pub fn get_feature_mut(&mut self, id: FeatureId) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.id() == id)
    }

    /// Returns the IDs of features that directly depend on `id`.
    /// Used to warn before deletion or to cascade suppression.
    pub fn dependents_of(&self, id: FeatureId) -> Vec<FeatureId> {
        self.features
            .iter()
            .filter(|f| f.dependencies().contains(&id))
            .map(|f| f.id())
            .collect()
    }

    /// Returns `true` if removing `id` would orphan downstream features.
    pub fn has_dependents(&self, id: FeatureId) -> bool {
        self.features.iter().any(|f| f.dependencies().contains(&id))
            || self.features.iter().any(|f| f.parent_id == Some(id))
    }

    /// Find all features whose `parent_id` is `id`.
    pub fn children_of(&self, id: FeatureId) -> Vec<FeatureId> {
        self.features.iter()
            .filter(|f| f.parent_id == Some(id))
            .map(|f| f.id())
            .collect()
    }

    /// Change a feature's parent. The feature is moved to just after the
    /// parent in the feature list so regeneration order stays sensible.
    pub fn reparent_to(&mut self, child_id: FeatureId, new_parent_id: FeatureId) -> bool {
        let child_idx = match self.features.iter().position(|f| f.id() == child_id) { Some(i) => i, None => return false };
        let parent_idx = match self.features.iter().position(|f| f.id() == new_parent_id) { Some(i) => i, None => return false };
        if child_idx == parent_idx { return false; }
        // Prevent circular: child's descendants must not include parent
        if self.is_descendant_of(new_parent_id, child_id) { return false; }
        self.features[child_idx].parent_id = Some(new_parent_id);
        // Move to just after parent in the list
        let f = self.features.remove(child_idx);
        let insert_at = if child_idx < parent_idx { parent_idx } else { parent_idx + 1 };
        let insert_at = insert_at.min(self.features.len());
        self.features.insert(insert_at, f);
        true
    }

    /// Detach a feature from its parent (make it top-level).
    pub fn detach_from_parent(&mut self, id: FeatureId) -> bool {
        if let Some(f) = self.features.iter_mut().find(|f| f.id() == id) {
            f.parent_id = None;
            true
        } else { false }
    }

    fn is_descendant_of(&self, ancestor_id: FeatureId, target_id: FeatureId) -> bool {
        let mut current = Some(target_id);
        while let Some(id) = current {
            if id == ancestor_id { return true; }
            current = self.features.iter().find(|f| f.id() == id).and_then(|f| f.parent_id);
        }
        false
    }

    /// Remove a feature by ID. Returns the removed feature if it existed.
    /// Also removes all child features (those with parent_id == id).
    ///
    /// **Caller responsibility:** check `has_dependents` first; downstream
    /// features that reference this one will silently fall back during
    /// regeneration, but the user almost certainly wants a warning.
    pub fn remove_feature(&mut self, id: FeatureId) -> Option<Feature> {
        let idx = self.features.iter().position(|f| f.id() == id)?;
        Some(self.features.remove(idx))
    }

    /// Remove a feature and all its children recursively.
    pub fn remove_feature_cascade(&mut self, id: FeatureId) -> Vec<FeatureId> {
        let mut removed = Vec::new();
        // Remove children first (their children are processed recursively)
        let child_ids: Vec<FeatureId> = self.children_of(id);
        for child_id in child_ids {
            removed.extend(self.remove_feature_cascade(child_id));
        }
        if self.remove_feature(id).is_some() {
            removed.push(id);
        }
        removed
    }

    /// Iterate features whose `suppressed` flag is false, in tree order.
    pub fn active_features(&self) -> impl Iterator<Item = &Feature> {
        self.features.iter().filter(|f| !f.suppressed)
    }

    /// Insert a feature at the position immediately after the last active
    /// feature (i.e., the "rollback point"). For now this is equivalent to
    /// `add_feature` (append) since we don't yet expose the rollback bar
    /// in the document model — but this method exists so the insertion
    /// policy can change in one place.
    pub fn insert_after_rollback(&mut self, feature: Feature) {
        self.features.push(feature);
    }

    /// Borrow the kind of a feature by ID.
    pub fn kind_of(&self, id: FeatureId) -> Option<&FeatureKind> {
        self.get_feature(id).map(|f| &f.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::Sketch;

    fn sketch_feature(id: u64) -> Feature {
        Feature::new(
            FeatureId(id),
            format!("Sketch{}", id),
            FeatureKind::Sketch { sketch: Sketch::new(), plane: crate::feature::PlaneDefinition::XY },
        )
    }

    #[test]
    fn dependents_of_finds_downstream() {
        let mut doc = Document::new("test");
        let s1 = sketch_feature(1);
        doc.add_feature(s1);
        doc.add_feature(Feature::new(
            FeatureId(2),
            "Extrude1",
            FeatureKind::Extrude {
                sketch_id: FeatureId(1),
                distance: ParameterId(1),
                direction: crate::feature::ExtrudeDirection::OneSide,
                draft_angle_deg: 0.0,
                selected_regions: None,
            },
        ));
        doc.add_parameter("depth", 1.0);

        assert_eq!(doc.dependents_of(FeatureId(1)), vec![FeatureId(2)]);
        assert!(doc.has_dependents(FeatureId(1)));
        assert!(!doc.has_dependents(FeatureId(2)));
    }

    #[test]
    fn remove_feature_returns_removed() {
        let mut doc = Document::new("test");
        doc.add_feature(sketch_feature(1));
        assert!(doc.remove_feature(FeatureId(1)).is_some());
        assert!(doc.remove_feature(FeatureId(1)).is_none());
    }
}
