//! Official EchoCAD gear plugin: involute spur gear generation.

mod gear;

use echi_core::sketch::Sketch;
use echi_plugin::{
    GeneratorInfo, ParamDef, Plugin, PluginError, ToolDefinition, ToolType,
    resolve_param,
};
use gear::{GearParams, generate_gear_sketch};
use std::collections::HashMap;

pub struct GearPlugin;

impl Plugin for GearPlugin {
    fn id(&self) -> &str {
        "echi.gear"
    }

    fn name(&self) -> &str {
        "Gear Generator"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn description(&self) -> &str {
        "Generate involute spur gear profiles. Parameters: module, teeth count, \
         pressure angle, addendum/dedendum coefficients."
    }

    fn generators(&self) -> Vec<GeneratorInfo> {
        vec![GeneratorInfo {
            id: "spur_gear".into(),
            name: "Spur Gear".into(),
            description: "Standard involute spur gear profile.".into(),
            icon: "gear".into(),
            parameters: vec![
                ParamDef {
                    id: "module".into(),
                    name: "Module".into(),
                    description: "Tooth size (mm).".into(),
                    default_value: 2.0,
                    min: Some(0.5),
                    max: Some(50.0),
                    step: 0.5,
                },
                ParamDef {
                    id: "teeth".into(),
                    name: "Teeth".into(),
                    description: "Number of teeth.".into(),
                    default_value: 20.0,
                    min: Some(5.0),
                    max: Some(200.0),
                    step: 1.0,
                },
                ParamDef {
                    id: "pressure_angle".into(),
                    name: "Pressure Angle".into(),
                    description: "Pressure angle in degrees (standard: 20).".into(),
                    default_value: 20.0,
                    min: Some(14.5),
                    max: Some(30.0),
                    step: 0.5,
                },
                ParamDef {
                    id: "addendum_coef".into(),
                    name: "Addendum Coef".into(),
                    description: "Addendum coefficient (1.0 = standard).".into(),
                    default_value: 1.0,
                    min: Some(0.5),
                    max: Some(2.0),
                    step: 0.1,
                },
                ParamDef {
                    id: "dedendum_coef".into(),
                    name: "Dedendum Coef".into(),
                    description: "Dedendum coefficient (1.25 = standard).".into(),
                    default_value: 1.25,
                    min: Some(0.5),
                    max: Some(2.5),
                    step: 0.1,
                },
            ],
        }]
    }

    fn generate_sketch(
        &self,
        generator_id: &str,
        params: &HashMap<String, f64>,
    ) -> Result<Sketch, PluginError> {
        match generator_id {
            "spur_gear" => {
                let gear_params = GearParams {
                    module: resolve_param(
                        params,
                        &self.generators()[0].parameters[0],
                    ),
                    teeth: resolve_param(
                        params,
                        &self.generators()[0].parameters[1],
                    ) as u32,
                    pressure_angle_deg: resolve_param(
                        params,
                        &self.generators()[0].parameters[2],
                    ),
                    addendum_coef: resolve_param(
                        params,
                        &self.generators()[0].parameters[3],
                    ),
                    dedendum_coef: resolve_param(
                        params,
                        &self.generators()[0].parameters[4],
                    ),
                    involute_points: 10,
                };

                if gear_params.teeth < 5 {
                    return Err(PluginError::InvalidParameter(
                        "Teeth must be at least 5".into(),
                    ));
                }

                Ok(generate_gear_sketch(&gear_params))
            }
            other => Err(PluginError::Generic(format!(
                "Unknown generator: {}",
                other
            ))),
        }
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            id: "gear_tool".into(),
            name: "Gear".into(),
            tool_type: ToolType::Sketch,
        }]
    }
}

/// Create a new instance of the gear plugin.
pub fn create_plugin() -> Box<dyn Plugin> {
    Box::new(GearPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_info() {
        let plugin = GearPlugin;
        let info = plugin.info();
        assert_eq!(info.id, "echi.gear");
        assert_eq!(info.generators.len(), 1);
        assert_eq!(info.generators[0].id, "spur_gear");
        assert_eq!(info.generators[0].parameters.len(), 5);
    }

    #[test]
    fn test_generate_gear() {
        let plugin = GearPlugin;
        let mut params = HashMap::new();
        params.insert("module".into(), 2.0);
        params.insert("teeth".into(), 20.0);
        let sketch = plugin.generate_sketch("spur_gear", &params).unwrap();
        assert!(sketch.entities.len() > 20, "Gear should have many entities");
    }
}
