# EchoCAD Architecture

This document describes the layered architecture of EchoCAD as of the
**0.4 iteration** (feature-kind split, dependency tracking, B-rep pipeline
behind feature flags, extrude region selection, plugin solid generation,
full Agent API parity).

## Client layers (API Parity — design principle #14)

Every modeling step is reachable through the Tauri command layer; the UI
is one client among several:

```
┌────────────────────────────────────────────────────────┐
│ Vue UI (human)        Agent API (AI agent)   Scripts    │
│ UnifiedViewport       app/src/commands/      tests      │
│ Toolbar / Panels      agent-api.ts (AgentCAD)           │
└───────────────────────────┬────────────────────────────┘
                            │ Tauri IPC
┌───────────────────────────▼────────────────────────────┐
│ app/src-tauri: commands.rs (thin shell), AppState,     │
│ UndoManager, autosave thread                           │
├────────────────────────────────────────────────────────┤
│ echi-render: regeneration pipeline (+ OCCT ops behind  │
│ `occt` feature)                                        │
├───────────┬──────────────┬──────────────┬──────────────┤
│ echi-core │ echi-geom    │ echi-brep    │ echi-plugin  │
│ Document  │ mesh kernels │ BrepKernel   │ Plugin trait │
│ Feature   │ (extrude,    │ trait + Mock │ + registry   │
│ Sketch    │ revolve,     │ + OCCT impl  │ + geo re-    │
│ Parameter │ boolean, …)  │ (cadrum)     │ exports      │
└───────────┴──────────────┴──────────────┴──────────────┘
```

## Crate dependency graph

```
              ┌──────────────┐
              │  echi-core   │  Document, Feature, Sketch, Parameter, PlaneDefinition
              └──────────────┘
                      ▲
            ┌─────────┼──────────┬────────────┐
            │         │          │            │
   ┌────────┴───┐  ┌──┴─────┐  ┌─┴────────┐  ┌┴────────────┐
   │ echi-geom  │  │ echi-  │  │ echi-    │  │ echi-plugin │
   │            │  │ render │  │ brep     │  │             │
   │ extrude /  │  │        │  │          │  │ Plugin trait │
   │ revolve /  │  │ regen  │  │ BrepKernel│  │ + registry  │
   │ sweep /    │  │ +occt_ │  │ Mock     │  │ + geometry  │
   │ fillet /   │  │  ops   │  │ Occt     │  │ re-exports  │
   │ boolean /  │  └───▲────┘  └────▲─────┘  └──────▲──────┘
   │ pattern /  │      │           │                │
   │ shell /    │      │      ┌────┴─────┐   ┌──────┴─────────┐
   │ solver     │      │      │ cadrum   │   │ echi-plugin-   │
   └─────┬──────┘      │      │ (OCCT)   │   │ gear/cycloidal │
         ▲             │      └──────────┘   └────────────────┘
         │             │
       ┌─┴─────────────┴──┐
       │   echicad-app    │  Tauri commands, AppState, UndoManager
       │ (app/src-tauri)  │
       └──────────────────┘
```

## Core data model

### `Feature` (echi-core/src/feature.rs)

`Feature` wraps a `FeatureKind` enum (14 variants):

```rust
pub struct Feature {
    pub id: FeatureId,
    pub name: String,
    pub suppressed: bool,
    pub color: Option<String>,   // per-feature display color, persisted
    pub kind: FeatureKind,       // flatten into the serde payload
}

pub enum FeatureKind {
    Sketch { sketch, plane },
    Extrude { sketch_id, distance, direction, draft_angle_deg, selected_regions },
    Revolve { sketch_id, angle, axis_entity_id },
    Fillet { target_id, radius, edges },
    Chamfer { target_id, distance, edges },
    Shell { target_id, thickness },
    Sweep { profile_sketch_id, path_sketch_id },
    Boolean { op, target_a, target_b },
    LinearPattern { target_id, dir, count, spacing },
    CircularPattern { target_id, axis_origin, axis_dir, count, total_angle_deg },
    Mirror { target_id, plane_normal, plane_point },
    CustomSketch { sketch, custom, plane },
    CustomSolid { sketch_id, distance, custom },
}
```

Key benefits:
- `id()`, `name()`, `set_name()` are trivial field accesses — no 14-arm match.
- `suppressed` is a first-class field (no separate suppression map).
- `dependencies()` returns the upstream feature IDs the regen engine uses to
  short-circuit propagation of failures.
- `edges` on Fillet/Chamfer carries user-picked edge selections; an empty
  list falls back to "all sharp edges".
- `selected_regions` on Extrude carries the user's loop selection from the
  region picker; `None` extrudes all loops.

### `Document` (echi-core/src/document.rs)

- `dependents_of(id)` — reverse lookup of references.
- `has_dependents(id)` — quick check used by the delete command.
- `active_features()` — iterator that skips suppressed features.
- `format_version` — bumped on layout changes; `echi-io` migrates older files.

## Regeneration pipeline

`echi-render::regenerate_with(doc, generator, brep_kernel, previous)` walks
`doc.features` in tree order. For each non-suppressed feature:

1. Verify every dependency exists in `doc` and wasn't already errored.
2. Dispatch on `FeatureKind`. Each solid kind tries the fallback chain
   **OCCT (feature `occt`) → B-rep kernel (feature `brep`) → echi-geom mesh**.
3. On error, record it in `RegenResult::errors` and skip downstream features
   that depend on the failed one.

When `previous` is provided, `RegenResult::compute_dirty` propagates the
changed feature's transitive dependents via BFS and only re-evaluates the
dirty set — clean features reuse cached meshes (incremental regeneration).

The frontend surfaces per-feature errors via `FeatureNode.errors` and
`get_regen_errors`.

## Extrude region selection

`echi_geom::extrude::extract_loops` returns every closed loop in a sketch
(circles are tessellated to 64-gons; line/arc chains are walked). The loops
are exposed to the frontend via `get_extrude_regions`, sorted by area
descending. The user picks a subset; the chosen indices are stored on the
feature as `selected_regions`.

At regeneration time `extrude_selected` filters the loops to the selection
and defers outer/hole classification to `extrude_loops`, which groups by
centroid containment: a smaller loop whose centroid lies inside a larger
loop becomes a hole of that loop; otherwise it is an independent outer.
This handles concentric circles, rectangles with circular holes, and
disjoint profiles uniformly.

## Tauri command conventions

All mutating Tauri commands follow this pattern (extracted into helpers
to remove per-command duplication):

```rust
fn add_feature_kind(state: &AppState, kind: FeatureKind) -> FeatureId {
    state.snapshot();                       // 1. record undo point
    let mut doc = state.lock_doc();
    let id = doc.new_feature_id();
    let name = default_feature_name(&kind, id);
    doc.add_feature(Feature::new(id, name, kind));
    regen_locked(&doc, &state);             // 2. re-evaluate the tree
    id
}
```

`add_feature_with_param` is the same with an extra `Parameter` allocation
for features that own a numeric parameter (extrude depth, fillet radius, …).

Command layer stays thin: business logic lives in crates (principle #9).

## Agent API

`app/src/commands/agent-api.ts` exposes an `AgentCAD` class (plus a `cad`
singleton) that mirrors the Tauri command surface with typed, documented
camelCase methods. It is the supported entry point for AI agents and
automation scripts. Coverage is kept at parity with the UI by design
principle #14 — the header comment of `agent-api.ts` carries the full
category/method table.

## Plugin system

Plugins implement `echi_plugin::Plugin` and register into a
`PluginRegistry` at startup. `echi-plugin` re-exports the entire geometry
surface (extrude, revolve, sweep, shell, fillet/chamfer, boolean,
pattern/mirror, mass properties) so plugins never depend on `echi-geom`
directly. Two reference plugins ship with the app: `echi-plugin-gear`
(involute spur gear, sketch + solid generation) and
`echi-plugin-cycloidal` （摆线针减速器 sketch generators).

`CustomSolid` features are evaluated during regeneration via the
`SolidGenerator` bridge: `PluginSolidGen` (in the app crate) adapts the
registry to `echi-render`'s `SolidGenerator` trait, avoiding a circular
crate dependency.

## File format

Project files are versioned JSON (`format_version: 1`):

```json
{
  "format_version": 1,
  "name": "Part1",
  "features": [
    { "id": 1, "name": "Sketch1", "suppressed": false, "type": "sketch", "sketch": { ... }, "plane": { "kind": "xy" } },
    { "id": 2, "name": "Extrude1", "suppressed": false, "type": "extrude", "sketch_id": 1, "distance": 1, "selected_regions": null }
  ],
  "parameters": [ { "id": 1, "name": "Extrude1", "value": 1.0 } ],
  "next_id": 3
}
```

Backward-compatibility: new fields use `#[serde(default)]` so v0 files
load unchanged; `echi-io::migrate_project` upgrades older versions.

## Autosave & recovery

A background thread writes the document to
`app_config_dir/autosave.echi` every 30 s. Writes are atomic
(`autosave.echi.tmp` → rename), and startup recovery validates the file
by attempting a full parse before offering restoration.

## Roadmap: Parametric B-Rep Pipeline

See [Parametric Refactor Plan](./parametric-refactor-plan.md) for the
detailed analysis and phased migration strategy. Current state:

- `echi-brep` crate ships with `BrepKernel` trait, `MockBrepKernel`
  (pure-Rust), and `OcctBrepKernel` (cadrum-backed, `occt` feature).
- All 14 feature kinds regenerate through the OCCT → B-rep → mesh
  fallback chain.
- STEP import/export and OCCT mass properties are available under the
  `occt` feature.
- The mesh pipeline remains the default; B-rep is toggled per-session
  via the toolbar switch.
