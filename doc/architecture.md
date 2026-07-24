# EchoCAD Architecture

This document describes the layered architecture of EchoCAD after the
**0.2 refactor** (feature-kind split, dependency tracking, dimension overlay,
multi-loop extrude, edge-detection fillet/chamfer, recent-files).

## Crate dependency graph

```
              ┌──────────────┐
              │  echi-core   │  Document, Feature, Sketch, Parameter, PlaneDefinition
              └──────────────┘
                      ▲
            ┌─────────┼──────────┐
            │         │          │
   ┌────────┴───┐  ┌──┴────┐  ┌──┴──────────┐
   │ echi-geom  │  │ echi- │  │ echi-plugin │
   │            │  │ render│  │             │
   │ extrude /  │  │       │  │ Plugin trait │
   │ revolve /  │  │ regen │  │ + registry  │
   │ sweep /    │  │       │  └─────────────┘
   │ fillet /   │  └───▲───┘         ▲
   │ boolean /  │      │             │
   │ pattern /  │      │      ┌──────┴─────────┐
   │ shell /    │      │      │ echi-plugin-   │
   │ solver     │      │      │ gear (example) │
   └─────┬──────┘      │      └────────────────┘
         ▲             │
         │             │
       ┌─┴─────────────┴──┐
       │   echicad-app    │  Tauri commands, AppState, UndoManager
       │ (app/src-tauri)  │
       └──────────────────┘
```

## Core data model

### `Feature` (echi-core/src/feature.rs)

After the 0.2 refactor, `Feature` is a wrapper around a `FeatureKind` enum:

```rust
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    pub suppressed: bool,
    pub kind: FeatureKind, // flatten into the serde payload
}

pub enum FeatureKind {
    Sketch { sketch, plane },
    Extrude { sketch_id, distance, direction, draft_angle_deg },
    Revolve { sketch_id, angle, axis_entity_id },
    Fillet { target_id, radius },
    Chamfer { target_id, distance },
    Shell { target_id, thickness },
    Sweep { profile_sketch_id, path_sketch_id },
    Boolean { op, target_a, target_b },
    LinearPattern { target_id, dir, count, spacing },
    CircularPattern { target_id, axis, count, total_angle_deg },
    Mirror { target_id, plane_normal, plane_point },
    CustomSketch { sketch, custom, plane },
    CustomSolid { sketch_id, distance, custom },
}
```

Key benefits:
- `id()`, `name()`, `set_name()` are trivial field accesses — no 12-arm match.
- `suppressed` is a first-class field (no separate suppression map).
- `dependencies()` returns the upstream feature IDs the regen engine uses to
  short-circuit propagation of failures.

### `Document` (echi-core/src/document.rs)

Adds:
- `dependents_of(id)` — reverse lookup of references.
- `has_dependents(id)` — quick check used by the delete command.
- `active_features()` — iterator that skips suppressed features.

## Regeneration pipeline

`echi-render::regenerate` walks `doc.features` in tree order. For each
non-suppressed feature:

1. Verify every dependency exists in `doc` and wasn't already errored.
2. Dispatch on `FeatureKind` to compute geometry.
3. On error, record it in `RegenResult::errors` and skip downstream features
   that depend on the failed one.

The frontend surfaces per-feature errors via `FeatureNode.errors` and
`get_regen_errors`.

## Frontend layering

```
            ┌─────────────────────────────────────────┐
            │                 HomeView                 │
            │  Toolbar, feature tree, properties panel │
            └────────────────────┬─────────────────────┘
                                 │
                ┌────────────────┴────────────────┐
                │                                 │
        ┌───────▼────────┐              ┌─────────▼────────┐
        │ UnifiedViewport │ ◄──────────► │ DimensionOverlay │
        │   (3D + sketch) │              │   (SVG overlay)  │
        └───────┬─────────┘              └──────────────────┘
                │
   ┌────────────┴─────────────┐
   │                          │
useThreeScene.ts        useSketchInteraction.ts
   │                          │
   │ Three.js scene, camera,  │ Snap rules, drawing state,
   │ orbit, plane projection  │ tool dispatch helpers
   │                          │
   └──────────────────────────┘
                │
         Pinia store (stores/sketch.ts)
                │
         Tauri commands (commands/sketch.ts)
                │
         Rust backend
```

## Tauri command conventions

All mutating Tauri commands follow this pattern (extracted into helpers
in 0.2 to remove the per-command duplication):

```rust
fn add_feature_kind(state: &AppState, kind: FeatureKind) -> FeatureId {
    state.snapshot();                       // 1. record undo point
    let mut doc = state.document.lock().unwrap();
    let id = doc.new_feature_id();
    let name = default_feature_name(&kind, id);
    doc.add_feature(Feature::new(id, name, kind));
    regen_locked(&doc, &state);             // 2. re-evaluate the tree
    id
}
```

`add_feature_with_param` is the same with an extra `Parameter` allocation
for features that own a numeric parameter (extrude depth, fillet radius, …).

## Multi-loop extrude

`echi_geom::extrude::extrude` now accepts sketches containing multiple
disjoint closed loops. The largest-area loop is treated as the outer
profile; everything else is treated as a hole. Triangulation uses
`triangulate_with_holes`, which bridges each hole into the outer ring
and then runs an indexed ear-clip with the even-odd rule.

## Edge-detection fillet/chamfer

`echi_geom::fillet` walks the mesh's edges, computes the dihedral angle
between adjacent faces, and identifies any edge with `dot(n_a, n_b) < 0.866`
(≈30°) as sharp. Each sharp edge is then beveled:

- **Chamfer** — single flat quad between offset copies of the edge endpoints.
- **Fillet** — N-segment arc strip with normals slerped between the two
  face normals.

This is a mesh-level approximation; true B-Rep fillets remain a future
enhancement.

## Driving dimension overlay

`DimensionOverlay.vue` is an SVG layer over the 3D viewport that draws
distance / radius / angle annotations from the active sketch's `Constraint`
list. Clicking a dimension opens an inline editor that re-issues the
constraint with the new value, then solves the sketch.

## Plugin system

Plugins implement the `Plugin` trait (`echi-plugin::types::Plugin`) and
expose `GeneratorInfo`/`ToolDefinition` metadata. The gear plugin
(`echi-plugin-gear`) is the reference implementation. Plugins are
registered at app startup; their generators appear in the toolbar's
"插件" menu.

## File format

Project files are JSON with the structure:

```json
{
  "name": "Part1",
  "features": [
    { "id": 1, "name": "Sketch1", "suppressed": false, "type": "sketch", "sketch": { ... }, "plane": { "kind": "xy" } },
    { "id": 2, "name": "Extrude1", "suppressed": false, "type": "extrude", "sketch_id": 1, "distance": 1, ... }
  ],
  "parameters": [
    { "id": 1, "name": "Extrude2", "value": 1.0 }
  ],
  "next_id": 3
}
```

Backward-compatibility: legacy files without `suppressed` default to
`false` via `#[serde(default)]`.

## Recent files

The Tauri backend persists a JSON list of up to 10 most-recent project
paths in `app_config_dir/recent_files.json`. The "文件 ▾" menu surfaces
them as a section at the bottom of the dropdown.

## Roadmap: Parametric B-Rep Pipeline

The current pure-triangle-mesh pipeline has fundamental limitations
(see [Parametric Refactor Plan](./parametric-refactor-plan.md) for
the detailed analysis and phased migration strategy).

Key points:
- All solid features currently produce `Mesh` (flat `positions/normals/indices`)
- Fillet/chamfer are mesh-level approximations; edge selections use raw vertex pairs
- The plan introduces an `echi-brep` crate with a `BrepKernel` trait
  and OpenCASCADE integration
- Migration follows the Strangler Fig pattern: B-rep and mesh pipelines
  coexist during the transition
- Estimated timeline: 22 weeks across 7 phases
