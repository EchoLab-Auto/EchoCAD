//! Thread-safe scene manager bridging the app (any thread) and the GPU
//! (main thread only).
//!
//! The app side calls `sync_from_regen(...)` after every regeneration; the
//! manager diffs against its known state and stages only the changed features
//! into a pending queue. The render plugin drains the queue on the main
//! thread each frame and uploads to the GPU. This is the "revision number +
//! changed ids" protocol from the migration plan: mesh bytes never cross IPC.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::scene::SceneMesh;

/// One staged mesh update, ready for main-thread upload.
#[derive(Debug, Clone, Default)]
pub struct StagedMesh {
    pub mesh: Option<SceneMesh>,
}

/// Thread-safe entry point held by `AppState`.
pub struct RenderManager {
    /// CPU-side scene cache (source of truth for bounds/highlight queries).
    scene: Mutex<HashMap<u32, SceneMesh>>,
    /// Features known to be current on the GPU.
    gpu_known: Mutex<HashSet<u32>>,
    /// Updates staged for the next frame (main thread drains this).
    pending_upsert: Mutex<Vec<(u32, SceneMesh)>>,
    pending_remove: Mutex<Vec<u32>>,
    /// Sketch overlay linework staged for the next frame (LineList positions).
    pending_sketch: Mutex<Option<Vec<f32>>>,
    /// Global revision bumped on every sync; consumed by the frontend
    /// (`viewport_updated` event carries it).
    rev: std::sync::atomic::AtomicU64,
    /// Incremental regen only produces dirty features; a full regen replaces
    /// everything. Both are handled by the same diff below.
    dirty_ids: Mutex<HashSet<u32>>,
}

impl Default for RenderManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderManager {
    pub fn new() -> Self {
        Self {
            scene: Mutex::new(HashMap::new()),
            gpu_known: Mutex::new(HashSet::new()),
            pending_upsert: Mutex::new(Vec::new()),
            pending_remove: Mutex::new(Vec::new()),
            pending_sketch: Mutex::new(None),
            rev: std::sync::atomic::AtomicU64::new(0),
            dirty_ids: Mutex::new(HashSet::new()),
        }
    }

    pub fn revision(&self) -> u64 {
        self.rev.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Replace the whole scene from a regeneration result. `solids` maps
    /// feature id → mesh data; features absent from `solids` are removed.
    ///
    /// Cheap: only changed features (by revision) are staged.
    pub fn sync_from_regen(&self, solids: &HashMap<u32, SceneMesh>, full_regen: bool) -> u64 {
        let mut scene = self.scene.lock().unwrap();
        let mut gpu_known = self.gpu_known.lock().unwrap();
        let mut upsert = self.pending_upsert.lock().unwrap();
        let mut remove = self.pending_remove.lock().unwrap();
        let mut dirty = self.dirty_ids.lock().unwrap();

        // Stage removals for features that vanished.
        let live: HashSet<u32> = solids.keys().copied().collect();
        let dead: Vec<u32> = if full_regen {
            scene.keys().filter(|id| !live.contains(id)).copied().collect()
        } else {
            dirty.iter().copied().filter(|id| !live.contains(id)).collect()
        };
        for id in &dead {
            scene.remove(id);
            gpu_known.remove(id);
            remove.push(*id);
        }

        // Stage upserts for new/changed features.
        for (id, mesh) in solids {
            let changed = if full_regen {
                scene.get(id).map_or(true, |prev| prev.positions.len() != mesh.positions.len() || prev.positions != mesh.positions)
            } else {
                dirty.contains(id)
            };
            if changed {
                scene.insert(*id, mesh.clone());
                gpu_known.insert(*id);
                upsert.push((*id, mesh.clone()));
            }
        }
        if !full_regen {
            dirty.clear();
        }

        let rev = self.rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        rev
    }

    /// Mark specific features dirty (incremental regen path).
    pub fn mark_dirty(&self, ids: impl IntoIterator<Item = u32>) {
        self.dirty_ids.lock().unwrap().extend(ids);
    }

    /// Stage new sketch overlay linework (world-space LineList positions).
    /// Empty clears the overlay.
    pub fn sync_sketch(&self, positions: Vec<f32>) {
        *self.pending_sketch.lock().unwrap() = Some(positions);
    }

    /// Drain staged updates and apply them to the renderer (main thread).
    /// Returns the number of GPU uploads performed (for logging).
    pub fn apply_pending(&self, renderer: &mut crate::renderer::Renderer) -> usize {
        let mut upsert = self.pending_upsert.lock().unwrap();
        let mut remove = self.pending_remove.lock().unwrap();
        let mut uploaded = 0;
        if !remove.is_empty() {
            for id in remove.drain(..) {
                renderer.remove_mesh(id);
            }
        }
        if !upsert.is_empty() {
            uploaded = upsert.len();
            for (id, mesh) in upsert.drain(..) {
                renderer.upsert_mesh(id, &mesh);
            }
        }
        // Sketch overlay.
        let mut sketch = self.pending_sketch.lock().unwrap();
        if let Some(positions) = sketch.take() {
            if positions.is_empty() {
                renderer.clear_sketch();
            } else {
                renderer.set_sketch(&positions);
            }
        }
        uploaded
    }

    /// World-space bounds of the CPU scene cache (for fitView).
    pub fn scene_bounds(&self) -> Option<(glam::Vec3, glam::Vec3)> {
        let scene = self.scene.lock().unwrap();
        crate::scene::scene_bounds(&scene)
    }

    /// Number of meshes in the CPU scene cache.
    pub fn scene_len(&self) -> usize {
        self.scene.lock().unwrap().len()
    }

    /// Remove everything (new document).
    pub fn clear(&self) {
        let mut scene = self.scene.lock().unwrap();
        let mut gpu_known = self.gpu_known.lock().unwrap();
        let mut upsert = self.pending_upsert.lock().unwrap();
        let mut remove = self.pending_remove.lock().unwrap();
        let _ = upsert.drain(..);
        remove.extend(scene.keys().copied());
        scene.clear();
        gpu_known.clear();
        self.rev.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    // ── Tests ──────────────────────────────────────────────────────────

    #[cfg(test)]
    fn pending_upsert_count(&self) -> usize {
        self.pending_upsert.lock().unwrap().len()
    }

    #[cfg(test)]
    fn pending_remove_count(&self) -> usize {
        self.pending_remove.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(n_verts: usize) -> SceneMesh {
        SceneMesh {
            positions: vec![0.0; n_verts * 3],
            normals: vec![1.0; n_verts * 3],
            indices: (0..n_verts as u32).collect(),
            label: String::new(),
            suppressed: false,
            error: None,
        }
    }

    #[test]
    fn full_sync_stages_new_meshes() {
        let m = RenderManager::new();
        let mut solids = HashMap::new();
        solids.insert(1, mesh(4));
        solids.insert(2, mesh(6));
        let rev = m.sync_from_regen(&solids, true);
        assert_eq!(rev, 1);
        assert_eq!(m.scene_len(), 2);
        assert_eq!(m.pending_upsert_count(), 2);
        assert_eq!(m.pending_remove_count(), 0);
    }

    #[test]
    fn unchanged_full_sync_stages_nothing() {
        let m = RenderManager::new();
        let mut solids = HashMap::new();
        solids.insert(1, mesh(4));
        m.sync_from_regen(&solids, true);
        // Same data again — no NEW pending updates (original 1 never drained).
        m.sync_from_regen(&solids, true);
        assert_eq!(m.pending_upsert_count(), 1);
        assert_eq!(m.revision(), 2);
    }

    #[test]
    fn changed_mesh_is_re_staged() {
        let m = RenderManager::new();
        let mut solids = HashMap::new();
        solids.insert(1, mesh(4));
        m.sync_from_regen(&solids, true);
        m.pending_upsert.lock().unwrap().clear();
        // Geometry changed (more vertices).
        solids.insert(1, mesh(9));
        m.sync_from_regen(&solids, true);
        assert_eq!(m.pending_upsert_count(), 1);
    }

    #[test]
    fn removed_feature_is_staged_for_removal() {
        let m = RenderManager::new();
        let mut solids = HashMap::new();
        solids.insert(1, mesh(4));
        solids.insert(2, mesh(4));
        m.sync_from_regen(&solids, true);
        m.pending_upsert.lock().unwrap().clear();
        solids.remove(&1);
        m.sync_from_regen(&solids, true);
        assert_eq!(m.pending_remove_count(), 1);
        assert_eq!(m.scene_len(), 1);
    }

    #[test]
    fn incremental_sync_only_stages_dirty() {
        let m = RenderManager::new();
        let mut solids = HashMap::new();
        solids.insert(1, mesh(4));
        solids.insert(2, mesh(4));
        m.sync_from_regen(&solids, true);
        m.pending_upsert.lock().unwrap().clear();
        // Incremental: feature 2 changed.
        m.mark_dirty([2]);
        m.sync_from_regen(&solids, false);
        assert_eq!(m.pending_upsert_count(), 1);
        assert_eq!(m.pending_upsert.lock().unwrap()[0].0, 2);
    }

    #[test]
    fn clear_stages_all_removals() {
        let m = RenderManager::new();
        let mut solids = HashMap::new();
        solids.insert(1, mesh(4));
        m.sync_from_regen(&solids, true);
        m.clear();
        assert_eq!(m.pending_remove_count(), 1);
        assert_eq!(m.scene_len(), 0);
    }
}
