use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParameterId(pub u64);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub id: ParameterId,
    pub name: String,
    pub value: f64,
}

impl Parameter {
    pub fn new(id: ParameterId, name: impl Into<String>, value: f64) -> Self {
        Self {
            id,
            name: name.into(),
            value,
        }
    }
}
