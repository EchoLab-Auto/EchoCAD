use echi_core::sketch::Sketch;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Plugin error: {0}")]
    Generic(String),
    #[error("Parameter missing: {0}")]
    MissingParameter(String),
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
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
