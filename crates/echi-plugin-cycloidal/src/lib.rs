//! EchoCAD cycloidal pin-wheel reducer plugin （摆线针减速器）.
//!
//! Provides two sketch generators:
//! - `cycloidal_wheel` — the cycloidal disc tooth profile + center bore
//! - `pin_wheel` — the pin ring housing + z2 pin holes

mod cycloidal;

use cycloidal::{CycloidalParams, generate_cycloidal_wheel, generate_pin_wheel};
use echi_core::sketch::Sketch;
use echi_plugin::{GeneratorInfo, ParamDef, Plugin, PluginError, ToolDefinition, ToolType};
use std::collections::HashMap;

pub struct CycloidalPlugin;

fn params_from(params: &HashMap<String, f64>, defs: &[ParamDef]) -> CycloidalParams {
    let get = |id: &str, default: f64| -> f64 {
        params.get(id).copied().unwrap_or_else(|| {
            defs.iter().find(|d| d.id == id).map(|d| d.default_value).unwrap_or(default)
        })
    };
    CycloidalParams {
        z_cycloidal: get("z_cycloidal", 11.0) as u32,
        pin_circle_radius: get("pin_circle_radius", 55.0),
        eccentricity: get("eccentricity", 2.0),
        pin_radius: get("pin_radius", 5.0),
        bore_diameter: get("bore_diameter", 20.0),
        ring_outer_diameter: get("ring_outer_diameter", 140.0),
        curve_points: get("curve_points", 360.0) as u32,
    }
}

fn shared_params() -> Vec<ParamDef> {
    vec![
        ParamDef {
            id: "z_cycloidal".into(),
            name: "摆线轮齿数 z₁".into(),
            description: "Cycloidal wheel teeth = reduction ratio. Pins = z₁ + 1.".into(),
            default_value: 11.0,
            min: Some(5.0),
            max: Some(87.0),
            step: 1.0,
        },
        ParamDef {
            id: "pin_circle_radius".into(),
            name: "针齿中心圆半径 R_p".into(),
            description: "Radius of the circle on which pins sit (mm).".into(),
            default_value: 55.0,
            min: Some(10.0),
            max: Some(500.0),
            step: 1.0,
        },
        ParamDef {
            id: "eccentricity".into(),
            name: "偏心距 e".into(),
            description: "Eccentricity (mm). Tooth height ≈ 2e.".into(),
            default_value: 2.0,
            min: Some(0.5),
            max: Some(10.0),
            step: 0.1,
        },
        ParamDef {
            id: "pin_radius".into(),
            name: "针齿半径 r_p".into(),
            description: "Radius of each pin (mm).".into(),
            default_value: 5.0,
            min: Some(1.0),
            max: Some(30.0),
            step: 0.5,
        },
    ]
}

impl Plugin for CycloidalPlugin {
    fn id(&self) -> &str {
        "echi.cycloidal"
    }

    fn name(&self) -> &str {
        "Cycloidal Reducer"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn description(&self) -> &str {
        "摆线针减速器：根据齿数（减速比）、针齿中心圆半径、偏心距、针齿半径 \
         生成摆线轮与针轮轮廓。"
    }

    fn generators(&self) -> Vec<GeneratorInfo> {
        let mut wheel_params = shared_params();
        wheel_params.push(ParamDef {
            id: "bore_diameter".into(),
            name: "中心孔径 d".into(),
            description: "Center bore diameter (mm).".into(),
            default_value: 20.0,
            min: Some(2.0),
            max: Some(200.0),
            step: 1.0,
        });
        wheel_params.push(ParamDef {
            id: "curve_points".into(),
            name: "曲线分段数".into(),
            description: "Sampling resolution of the tooth curve.".into(),
            default_value: 360.0,
            min: Some(64.0),
            max: Some(1440.0),
            step: 16.0,
        });

        let mut ring_params = shared_params();
        ring_params.push(ParamDef {
            id: "ring_outer_diameter".into(),
            name: "针轮外径 D".into(),
            description: "Outer diameter of the pin wheel housing (mm).".into(),
            default_value: 140.0,
            min: Some(20.0),
            max: Some(1000.0),
            step: 2.0,
        });

        vec![
            GeneratorInfo {
                id: "cycloidal_wheel".into(),
                name: "摆线轮".into(),
                description: "Cycloidal disc tooth profile (epitrochoid) with center bore.".into(),
                icon: "gear".into(),
                parameters: wheel_params,
            },
            GeneratorInfo {
                id: "pin_wheel".into(),
                name: "针轮".into(),
                description: "Pin wheel housing: outer ring + equally spaced pin holes.".into(),
                icon: "gear".into(),
                parameters: ring_params,
            },
        ]
    }

    fn generate_sketch(
        &self,
        generator_id: &str,
        params: &HashMap<String, f64>,
    ) -> Result<Sketch, PluginError> {
        let defs = self.generators();
        let defs_ref: &[ParamDef] = match generator_id {
            "cycloidal_wheel" => &defs[0].parameters,
            "pin_wheel" => &defs[1].parameters,
            other => {
                return Err(PluginError::Generic(format!("Unknown generator: {}", other)));
            }
        };
        let p = params_from(params, defs_ref);
        p.validate().map_err(|s| PluginError::invalid_param(s))?;

        match generator_id {
            "cycloidal_wheel" => Ok(generate_cycloidal_wheel(&p)),
            "pin_wheel" => Ok(generate_pin_wheel(&p)),
            other => Err(PluginError::Generic(format!("Unknown generator: {}", other))),
        }
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            id: "cycloidal_tool".into(),
            name: "Cycloidal".into(),
            tool_type: ToolType::Sketch,
        }]
    }
}

/// Create a new instance of the cycloidal reducer plugin.
pub fn create_plugin() -> Box<dyn Plugin> {
    Box::new(CycloidalPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_metadata() {
        let plugin = CycloidalPlugin;
        let info = plugin.info();
        assert_eq!(info.id, "echi.cycloidal");
        assert_eq!(info.generators.len(), 2);
        assert_eq!(info.generators[0].id, "cycloidal_wheel");
        assert_eq!(info.generators[1].id, "pin_wheel");
    }

    #[test]
    fn generate_both_wheels() {
        let plugin = CycloidalPlugin;
        let params = HashMap::new();
        let w = plugin.generate_sketch("cycloidal_wheel", &params).unwrap();
        assert!(w.entities.len() > 100);
        let r = plugin.generate_sketch("pin_wheel", &params).unwrap();
        assert!(r.entities.len() > 100);
    }

    #[test]
    fn rejects_invalid_params() {
        let plugin = CycloidalPlugin;
        let mut params = HashMap::new();
        params.insert("eccentricity".into(), 100.0);
        assert!(plugin.generate_sketch("cycloidal_wheel", &params).is_err());
    }
}
