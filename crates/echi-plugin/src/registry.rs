use crate::{GeneratorInfo, ParamDef, Plugin, PluginError, PluginInfo, ToolDefinition};
use echi_core::sketch::Sketch;
use echi_geom::Mesh;
use std::collections::HashMap;

/// Registry that holds all loaded plugins and provides lookup.
pub struct PluginRegistry {
    plugins: Vec<Box<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Register a plugin. Called during app initialization.
    pub fn register(&mut self, mut plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        log::info!("Registering plugin: {} v{}", plugin.id(), plugin.version());
        plugin.on_init()?;
        self.plugins.push(plugin);
        Ok(())
    }

    /// Get metadata for all registered plugins.
    pub fn list_plugins(&self) -> Vec<PluginInfo> {
        self.plugins.iter().map(|p| p.info()).collect()
    }

    /// Get all feature generators from all plugins.
    pub fn list_generators(&self) -> Vec<(String, GeneratorInfo)> {
        let mut generators = Vec::new();
        for plugin in &self.plugins {
            let plugin_id = plugin.id().to_string();
            for generator in plugin.generators() {
                generators.push((plugin_id.clone(), generator));
            }
        }
        generators
    }

    /// Get all tools from all plugins.
    pub fn list_tools(&self) -> Vec<(String, ToolDefinition)> {
        let mut tools = Vec::new();
        for plugin in &self.plugins {
            let plugin_id = plugin.id().to_string();
            for tool in plugin.tools() {
                tools.push((plugin_id.clone(), tool));
            }
        }
        tools
    }

    /// Find a generator by plugin and generator ID.
    pub fn find_generator(&self, plugin_id: &str, generator_id: &str) -> Option<GeneratorInfo> {
        for plugin in &self.plugins {
            if plugin.id() == plugin_id {
                return plugin.generators().into_iter().find(|g| g.id == generator_id);
            }
        }
        None
    }

    /// Generate a sketch from a plugin's feature generator.
    pub fn generate_sketch(
        &self,
        plugin_id: &str,
        generator_id: &str,
        params: &HashMap<String, f64>,
    ) -> Result<Sketch, PluginError> {
        for plugin in &self.plugins {
            if plugin.id() == plugin_id {
                return plugin.generate_sketch(generator_id, params);
            }
        }
        Err(PluginError::Generic(format!(
            "Plugin '{}' not found",
            plugin_id
        )))
    }

    /// Generate a solid mesh from a plugin's feature generator.
    /// Mirrors [`generate_sketch`](Self::generate_sketch) but dispatches to
    /// [`Plugin::generate_solid`]. Plugins that don't override the solid
    /// method return their default "not supported" error, which the caller
    /// is expected to surface (design principle #8 — errors must be visible).
    pub fn generate_solid(
        &self,
        plugin_id: &str,
        generator_id: &str,
        params: &HashMap<String, f64>,
    ) -> Result<Mesh, PluginError> {
        for plugin in &self.plugins {
            if plugin.id() == plugin_id {
                return plugin.generate_solid(generator_id, params);
            }
        }
        Err(PluginError::Generic(format!(
            "Plugin '{}' not found",
            plugin_id
        )))
    }

    /// Lookup parameter definitions for a generator.
    pub fn generator_params(&self, plugin_id: &str, generator_id: &str) -> Vec<ParamDef> {
        self.find_generator(plugin_id, generator_id)
            .map(|g| g.parameters.clone())
            .unwrap_or_default()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}
