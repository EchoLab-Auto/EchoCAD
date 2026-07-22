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
    }

    /// Remove a feature by ID. Returns the removed feature if it existed.
    ///
    /// **Caller responsibility:** check `has_dependents` first; downstream
    /// features that reference this one will silently fall back during
    /// regeneration, but the user almost certainly wants a warning.
    pub fn remove_feature(&mut self, id: FeatureId) -> Option<Feature> {
        let idx = self.features.iter().position(|f| f.id() == id)?;
        Some(self.features.remove(idx))
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
