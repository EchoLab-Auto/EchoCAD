//! EchoCAD project file IO and serialization.

use echi_core::Document;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Save a document to a project file.
pub fn save_project(doc: &Document, path: impl AsRef<Path>) -> Result<(), ProjectError> {
    let json = serde_json::to_string_pretty(doc)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Load a document from a project file.
pub fn load_project(path: impl AsRef<Path>) -> Result<Document, ProjectError> {
    let bytes = std::fs::read(path)?;
    let doc = serde_json::from_slice(&bytes)?;
    Ok(doc)
}
