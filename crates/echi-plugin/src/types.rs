use echi_core::sketch::Sketch;
use echi_geom::Mesh;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Plugin error: {0}")]
    Generic(String),
    #[error("Parameter missing: {0}")]
    MissingParameter(String),
    #[error("Invalid parameter: {reason}")]
    InvalidParameter { name: String, reason: String },
    #[error("Geometry error: {0}")]
    Geometry(String),
    #[error("Generator '{0}' not found")]
    GeneratorNotFound(String),
    #[error("Operation not supported: {0}")]
    Unsupported(String),
}

impl PluginError {
    /// Convenience: create an `InvalidParameter` error with a combined message.
    /// For backward compatibility when callers pass a single string.
    pub fn invalid_param(reason: impl Into<String>) -> Self {
        PluginError::InvalidParameter {
            name: String::new(),
            reason: reason.into(),
        }
    }
}

/// Describes a parameter accepted by a feature generator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: f64,
}

/// Defines a tool that appears in the UI toolbar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub id: String,
    pub name: String,
    pub tool_type: ToolType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolType {
    Sketch,
    Solid,
}

/// Serializable plugin info sent to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub generators: Vec<GeneratorInfo>,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub parameters: Vec<ParamDef>,
}

/// The core plugin trait. Implement this to extend EchoCAD.
///
/// All methods have default no-op implementations so plugins only
/// need to override what they actually provide.
pub trait Plugin: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn description(&self) -> &str;

    /// Called once when the plugin is loaded.
    fn on_init(&mut self) -> Result<(), PluginError> {
        Ok(())
    }

    /// Feature generators this plugin provides (e.g. "gear", "spring").
    fn generators(&self) -> Vec<GeneratorInfo> {
        Vec::new()
    }

    /// Generate the sketch for a feature generator.
    /// Returns the entities and constraints that form the generated sketch.
    fn generate_sketch(
        &self,
        generator_id: &str,
        params: &HashMap<String, f64>,
    ) -> Result<Sketch, PluginError> {
        let _ = (generator_id, params);
        Err(PluginError::Generic("not implemented".into()))
    }

    /// Generate a solid (triangle mesh) directly from a feature generator.
    ///
    /// This is the solid-body counterpart to [`generate_sketch`](Self::generate_sketch):
    /// plugins that can produce 3D geometry (e.g. an extruded gear body)
    /// override this. The default implementation returns an error so that
    /// plugins written against the older sketch-only API still compile and
    /// simply report "not supported" when asked for a solid.
    fn generate_solid(
        &self,
        generator_id: &str,
        params: &HashMap<String, f64>,
    ) -> Result<Mesh, PluginError> {
        let _ = (generator_id, params);
        Err(PluginError::Generic("solid generation not supported".into()))
    }

    /// UI tools this plugin contributes.
    fn tools(&self) -> Vec<ToolDefinition> {
        Vec::new()
    }

    /// Collect plugin metadata for frontend consumption.
    fn info(&self) -> PluginInfo {
        PluginInfo {
            id: self.id().to_string(),
            name: self.name().to_string(),
            version: self.version().to_string(),
            description: self.description().to_string(),
            generators: self.generators(),
            tools: self.tools(),
        }
    }
}

/// Helper to resolve parameter values with defaults.
pub fn resolve_param(params: &HashMap<String, f64>, def: &ParamDef) -> f64 {
    params.get(&def.id).copied().unwrap_or(def.default_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal plugin that only implements `generate_solid`, used to verify
    /// the registry passthrough dispatches correctly.
    struct StubSolidPlugin;

    impl Plugin for StubSolidPlugin {
        fn id(&self) -> &str { "stub.solid" }
        fn name(&self) -> &str { "Stub Solid" }
        fn version(&self) -> &str { "0.0.1" }
        fn description(&self) -> &str { "test plugin" }

        fn generate_solid(
            &self,
            generator_id: &str,
            _params: &HashMap<String, f64>,
        ) -> Result<Mesh, PluginError> {
            if generator_id != "cube" {
                return Err(PluginError::Generic(format!(
                    "unknown generator: {}",
                    generator_id
                )));
            }
            // Trivial two-triangle quad in the XY plane — enough to prove the
            // mesh travelled through the dispatch unchanged.
            Ok(Mesh {
                positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
                normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
                indices: vec![0, 1, 2, 0, 2, 3],
            })
        }
    }

    #[test]
    fn default_generate_solid_returns_not_supported() {
        // A plugin that doesn't override generate_solid must still compile and
        // return the canonical "not supported" error.
        struct SketchOnly;
        impl Plugin for SketchOnly {
            fn id(&self) -> &str { "sketch.only" }
            fn name(&self) -> &str { "SketchOnly" }
            fn version(&self) -> &str { "0.0.1" }
            fn description(&self) -> &str { "no solid" }
        }
        let plugin = SketchOnly;
        let params = HashMap::new();
        let err = plugin.generate_solid("anything", &params).unwrap_err();
        match err {
            PluginError::Generic(msg) => assert!(msg.contains("not supported"), "got: {}", msg),
            other => panic!("expected Generic, got {:?}", other),
        }
    }

    #[test]
    fn registry_generate_solid_dispatches_to_plugin() {
        let mut registry = crate::PluginRegistry::new();
        registry.register(Box::new(StubSolidPlugin)).unwrap();
        let params = HashMap::new();
        let mesh = registry.generate_solid("stub.solid", "cube", &params).expect("mesh");
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.indices.len(), 6);
    }

    #[test]
    fn registry_generate_solid_errors_when_plugin_missing() {
        let registry = crate::PluginRegistry::new();
        let params = HashMap::new();
        let err = registry.generate_solid("does.not.exist", "cube", &params).unwrap_err();
        assert!(matches!(err, PluginError::Generic(_)));
    }

    #[test]
    fn registry_generate_solid_propagates_generator_error() {
        let mut registry = crate::PluginRegistry::new();
        registry.register(Box::new(StubSolidPlugin)).unwrap();
        let params = HashMap::new();
        // Plugin exists but the generator id is wrong → its error must surface.
        let err = registry.generate_solid("stub.solid", "missing_gen", &params).unwrap_err();
        match err {
            PluginError::Generic(msg) => assert!(msg.contains("unknown generator"), "got: {}", msg),
            other => panic!("expected Generic, got {:?}", other),
        }
    }
}
