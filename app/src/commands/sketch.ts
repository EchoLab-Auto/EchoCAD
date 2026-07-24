import { invoke } from "@tauri-apps/api/core";

export type EntityId = number;
export type FeatureId = number;
export type ParameterId = number;

export interface SketchPointEntity {
  type: "Point";
  id: EntityId;
  x: number;
  y: number;
}

export interface SketchLineEntity {
  type: "Line";
  id: EntityId;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  construction?: boolean;
}

export interface SketchCircleEntity {
  type: "Circle";
  id: EntityId;
  cx: number;
  cy: number;
  radius: number;
  construction?: boolean;
}

export interface SketchArcEntity {
  type: "Arc";
  id: EntityId;
  cx: number;
  cy: number;
  radius: number;
  start_angle: number;
  end_angle: number;
}

export interface SketchSplineEntity {
  type: "Spline";
  id: EntityId;
  points: [number, number][];
}

export interface SketchEllipseEntity {
  type: "Ellipse";
  id: EntityId;
  cx: number;
  cy: number;
  major_rx: number;
  major_ry: number;
  ratio: number;
}

export type SketchEntity =
  | SketchPointEntity
  | SketchLineEntity
  | SketchCircleEntity
  | SketchArcEntity
  | SketchSplineEntity
  | SketchEllipseEntity;

export type Constraint =
  | { Coincident: { a: EntityId; b: EntityId } }
  | { Horizontal: { line: EntityId } }
  | { Vertical: { line: EntityId } }
  | { Distance: { a: EntityId; b: EntityId; distance: number } }
  | { Radius: { circle: EntityId; radius: number } }
  | { Parallel: { line_a: EntityId; line_b: EntityId } }
  | { Perpendicular: { line_a: EntityId; line_b: EntityId } }
  | { Tangent: { line: EntityId; circle: EntityId } }
  | { Concentric: { a: EntityId; b: EntityId } }
  | { Equal: { a: EntityId; b: EntityId } }
  | { Fix: { point: EntityId } }
  | { Midpoint: { point: EntityId; line: EntityId } }
  | { Symmetric: { a: EntityId; b: EntityId; axis: EntityId } }
  | { PointOnLine: { point: EntityId; line: EntityId } }
  | { Collinear: { a: EntityId; b: EntityId; c: EntityId } }
  | { Angle: { line_a: EntityId; line_b: EntityId; angle_deg: number } }
  | { Diameter: { circle: EntityId; diameter: number } };

export interface ParameterInfo {
  id: ParameterId;
  name: string;
  value: number;
  readonly?: boolean;
}

export interface FeatureNode {
  id: FeatureId;
  name: string;
  feature_type: "Sketch" | "Extrude" | "Revolve" | string;
  parameters: ParameterInfo[];
  suppressed: boolean;
  has_dependents: boolean;
  errors: string | null;
  /** For sketches: the plane the sketch lives on. */
  plane: "xy" | "yz" | "zx" | "offset" | null;
  /** Optional per-feature color as a hex string (e.g. "#ff8800").
   *  `null` / absent means the renderer should use the default palette. */
  color?: string | null;
}

export interface RenderMesh {
  positions: number[];
  normals: number[];
  indices: number[];
}

export async function getFeatures(): Promise<FeatureNode[]> {
  return invoke("get_features");
}

export async function addSketchFeature(plane?: string): Promise<FeatureId> {
  return invoke("add_sketch_feature", { plane: plane || "xy" });
}

export async function setActiveSketch(id: FeatureId): Promise<void> {
  return invoke("set_active_sketch", { id });
}

export async function addExtrudeFeature(
  sketchId: FeatureId,
  direction?: string,
  dist2?: number,
  draftAngleDeg?: number,
  depth?: number,
  selectedRegions?: number[] | null,
): Promise<FeatureId> {
  return invoke("add_extrude_feature", {
    sketchId,
    direction: direction || "one_side",
    dist2: dist2 || 1.0,
    draftAngleDeg: draftAngleDeg || 0.0,
    depth: depth || 1.0,
    selectedRegions: selectedRegions ?? null,
  });
}

export interface ExtrudeRegionInfo {
  index: number;
  outer: [number, number][];
  area: number;
}

export async function getExtrudeRegions(): Promise<ExtrudeRegionInfo[]> {
  return invoke("get_extrude_regions");
}

export async function previewExtrude(
  sketchId: FeatureId,
  direction: string,
  dist2: number,
  draftAngleDeg: number,
  depth: number,
  selectedRegions?: number[] | null,
): Promise<RenderMesh | null> {
  return invoke("preview_extrude", {
    sketchId,
    direction: direction || "one_side",
    dist2: dist2 || 1.0,
    draftAngleDeg: draftAngleDeg || 0.0,
    depth: depth || 1.0,
    selectedRegions: selectedRegions ?? null,
  });
}

export async function addFilletFeature(
  targetId: FeatureId,
  radius: number
): Promise<FeatureId> {
  return invoke("add_fillet_feature", { targetId, radius });
}

/// Fillet a specific selection of edges on the target solid. Each edge is a
/// `(vertex_a, vertex_b)` index pair into the target solid's mesh. An empty
/// list falls back to "all sharp edges" (same as `addFilletFeature`).
export async function addFilletEdgesFeature(
  targetId: FeatureId,
  radius: number,
  edges: [number, number][]
): Promise<FeatureId> {
  return invoke("add_fillet_edges_feature", { targetId, radius, edges });
}

export async function addLinearPattern(
  targetId: FeatureId, dirX: number, dirY: number, dirZ: number,
  count: number, spacing: number
): Promise<FeatureId> {
  return invoke("add_linear_pattern", { targetId, dirX, dirY, dirZ, count, spacing });
}

export async function addCircularPattern(
  targetId: FeatureId,
  axisX: number, axisY: number, axisZ: number,
  axisDx: number, axisDy: number, axisDz: number,
  count: number, totalAngleDeg: number
): Promise<FeatureId> {
  return invoke("add_circular_pattern", {
    targetId, axisX, axisY, axisZ, axisDx, axisDy, axisDz, count, totalAngleDeg,
  });
}

export async function addMirrorFeature(
  targetId: FeatureId,
  planeNx: number, planeNy: number, planeNz: number,
  planePx: number, planePy: number, planePz: number,
): Promise<FeatureId> {
  return invoke("add_mirror_feature", {
    targetId, planeNx, planeNy, planeNz, planePx, planePy, planePz,
  });
}

export async function addSweepFeature(
  profileId: FeatureId, pathId: FeatureId
): Promise<FeatureId> {
  return invoke("add_sweep_feature", { profileId, pathId });
}

export async function addBooleanFeature(
  targetA: FeatureId, targetB: FeatureId, op: string
): Promise<FeatureId> {
  return invoke("add_boolean_feature", { targetA, targetB, op });
}

export async function addShellFeature(
  targetId: FeatureId, thickness: number
): Promise<FeatureId> {
  return invoke("add_shell_feature", { targetId, thickness });
}

export async function addChamferFeature(
  targetId: FeatureId,
  distance: number
): Promise<FeatureId> {
  return invoke("add_chamfer_feature", { targetId, distance });
}

/// Chamfer a specific selection of edges on the target solid. See
/// [`addFilletEdgesFeature`] for the edge-pair convention; an empty list
/// falls back to "all sharp edges".
export async function addChamferEdgesFeature(
  targetId: FeatureId,
  distance: number,
  edges: [number, number][]
): Promise<FeatureId> {
  return invoke("add_chamfer_edges_feature", { targetId, distance, edges });
}

/// Create a new sketch on a plane offset from one of the three base planes.
/// `basePlaneTag` is `"xy"` | `"yz"` | `"zx"`; `distance` is a signed offset
/// along the base plane's normal. The new sketch becomes the active sketch.
export async function createOffsetPlane(
  basePlaneTag: string,
  distance: number
): Promise<FeatureId> {
  return invoke("create_offset_plane", { basePlaneTag, distance });
}

export async function addRevolveFeature(
  sketchId: FeatureId,
  axisEntityId?: EntityId | null
): Promise<FeatureId> {
  return invoke("add_revolve_feature", {
    sketchId,
    axisEntityId: axisEntityId ?? null,
  });
}

export async function updateParameter(
  id: ParameterId,
  value: number
): Promise<void> {
  return invoke("update_parameter", { id, value });
}

export async function renameFeature(id: FeatureId, name: string): Promise<void> {
  return invoke("rename_feature", { id, name });
}

export async function setFeatureSuppressed(
  id: FeatureId,
  suppressed: boolean
): Promise<void> {
  return invoke("set_feature_suppressed", { id, suppressed });
}

export async function setFeatureColor(featureId: number, color: string | null): Promise<void> {
  return invoke("set_feature_color", { featureId, color });
}

export async function deleteFeature(
  id: FeatureId,
  cascade?: boolean
): Promise<void> {
  return invoke("delete_feature", { id, cascade: cascade ?? false });
}

export async function getSketchEntities(): Promise<SketchEntity[]> {
  return invoke("get_sketch_entities");
}

export async function getSketchConstraints(): Promise<Constraint[]> {
  return invoke("get_sketch_constraints");
}

export async function removeConstraint(index: number): Promise<boolean> {
  return invoke("remove_constraint", { index });
}

export async function addPoint(x: number, y: number): Promise<EntityId | null> {
  return invoke("add_point", { x, y });
}

export async function addLine(
  start: EntityId,
  end: EntityId
): Promise<EntityId | null> {
  return invoke("add_line", { start, end });
}

export async function addCircle(
  center: EntityId,
  radius: number
): Promise<EntityId | null> {
  return invoke("add_circle", { center, radius });
}

export async function addSpline(
  controlPoints: EntityId[]
): Promise<EntityId | null> {
  return invoke("add_spline", { controlPoints });
}

export async function addEllipse(
  center: EntityId,
  majorAxisEnd: EntityId,
  ratio: number
): Promise<EntityId | null> {
  return invoke("add_ellipse", { center, majorAxisEnd, ratio });
}

export async function addArc(
  center: EntityId,
  radius: number,
  startAngle: number,
  endAngle: number
): Promise<EntityId | null> {
  return invoke("add_arc", { center, radius, startAngle, endAngle });
}

export async function addConstraint(constraint: Constraint): Promise<void> {
  return invoke("add_constraint", { constraint });
}

/// Update the numeric value of an existing constraint in place.
/// Use this for editing driving dimensions — plain `addConstraint` would
/// accumulate duplicates that fight each other in the solver.
export async function updateConstraintValue(constraint: Constraint): Promise<boolean> {
  return invoke("update_constraint_value", { constraint });
}

export async function solveSketch(): Promise<string[]> {
  return invoke("solve_sketch");
}

export async function updateEntityProp(
  id: EntityId, prop: string, value: number
): Promise<boolean> {
  return invoke("update_entity_prop", { id, prop, value });
}

export async function deleteEntity(id: EntityId): Promise<boolean> {
  return invoke("delete_entity", { id });
}

export async function movePoint(
  id: EntityId,
  x: number,
  y: number
): Promise<void> {
  return invoke("move_point", { id, x, y });
}

/// Move a point without creating an undo snapshot (for drag intermediates).
export async function movePointNoSnapshot(
  id: EntityId,
  x: number,
  y: number
): Promise<void> {
  return invoke("move_point_no_snapshot", { id, x, y });
}

export async function clearSketch(): Promise<void> {
  return invoke("clear_sketch");
}

export async function getSolidMesh(): Promise<RenderMesh | null> {
  return invoke("get_solid_mesh");
}

export interface SolidMeshEntry {
  feature_id: FeatureId;
  name: string;
  mesh: RenderMesh;
  suppressed: boolean;
  error: string | null;
}

export async function getAllSolidMeshes(): Promise<SolidMeshEntry[]> {
  return invoke("get_all_solid_meshes");
}

export async function getRegenErrors(): Promise<Array<[FeatureId, string]>> {
  return invoke("get_regen_errors");
}

export async function getUseBrep(): Promise<boolean> {
  return invoke("get_use_brep");
}

export async function setUseBrep(value: boolean): Promise<void> {
  return invoke("set_use_brep", { value });
}

export async function captureViewport(): Promise<string> {
  return invoke("capture_viewport");
}

export async function saveProject(): Promise<string> {
  return invoke("save_project_cmd");
}

export async function saveProjectTo(path: string): Promise<void> {
  return invoke("save_project_to", { path });
}

export async function loadProject(): Promise<void> {
  return invoke("load_project_cmd");
}

export async function loadProjectFrom(path: string): Promise<void> {
  return invoke("load_project_from", { path });
}

export async function getRecentFiles(): Promise<string[]> {
  return invoke("get_recent_files");
}

export async function clearRecentFiles(): Promise<boolean> {
  return invoke("clear_recent_files");
}

export async function exportStl(): Promise<string> {
  return invoke("export_stl");
}

export async function exportObj(): Promise<string> {
  return invoke("export_obj");
}

export async function exportGltf(): Promise<string> {
  return invoke("export_gltf_cmd");
}

export async function checkRecovery(): Promise<boolean> {
  return invoke("check_recovery");
}

export async function clearDocument(): Promise<void> {
  return invoke("clear_document");
}

export async function undo(): Promise<boolean> {
  return invoke("undo");
}

export async function redo(): Promise<boolean> {
  return invoke("redo");
}

export async function canUndoRedo(): Promise<[boolean, boolean]> {
  return invoke("can_undo_redo");
}

// ── Plugin commands ──────────────────────────────────────────────

export interface ParamDef {
  id: string;
  name: string;
  description: string;
  default_value: number;
  min: number | null;
  max: number | null;
  step: number;
}

export interface GeneratorInfo {
  plugin_id: string;
  id: string;
  name: string;
  description: string;
  icon: string;
  parameters: ParamDef[];
}

export interface ToolDef {
  plugin_id: string;
  id: string;
  name: string;
  tool_type: "sketch" | "solid";
}

export interface PluginInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  generators: GeneratorInfo[];
  tools: ToolDef[];
}

export async function listPlugins(): Promise<PluginInfo[]> {
  return invoke("list_plugins");
}

export async function listGenerators(): Promise<GeneratorInfo[]> {
  return invoke("list_generators");
}

export async function generatePluginFeature(
  pluginId: string,
  generatorId: string,
  params: Record<string, number>
): Promise<FeatureId> {
  return invoke("generate_plugin_feature", {
    pluginId,
    generatorId,
    params,
  });
}

// ── Measurement commands ──────────────────────────────────────────

/** Compute the Euclidean distance between two 3D world-space points.
 *  Idempotent — no document mutation. */
export async function measureDistance(
  ax: number, ay: number, az: number,
  bx: number, by: number, bz: number,
): Promise<number> {
  return invoke("measure_distance", { ax, ay, az, bx, by, bz });
}

/** Compute the angle <p1-p2-p3 in degrees.
 *  Idempotent — no document mutation. */
export async function measureAngle(
  p1x: number, p1y: number, p1z: number,
  p2x: number, p2y: number, p2z: number,
  p3x: number, p3y: number, p3z: number,
): Promise<number> {
  return invoke("measure_angle", { p1x, p1y, p1z, p2x, p2y, p2z, p3x, p3y, p3z });
}

// ── Mass properties ─────────────────────────────────────────────

export interface MassProperties {
  volume: number;
  surface_area: number;
  centroid: [number, number, number];
}

/** Compute mass properties for a feature's solid mesh.
 *  Idempotent — no document mutation. */
export async function getMassProperties(featureId: FeatureId): Promise<MassProperties | null> {
  return invoke("get_mass_properties", { featureId });
}

// ── Pattern / Mirror parameter update commands ────────────────────

/** Update parameters of an existing LinearPattern feature.
 *  Document mutation — triggers snapshot + regen. */
export async function updateLinearPattern(
  featureId: FeatureId,
  count: number,
  spacing: number,
  dirX: number, dirY: number, dirZ: number,
): Promise<void> {
  return invoke("update_linear_pattern", {
    featureId, count, spacing, dirX, dirY, dirZ,
  });
}

/** Update parameters of an existing CircularPattern feature.
 *  Document mutation — triggers snapshot + regen. */
export async function updateCircularPattern(
  featureId: FeatureId,
  count: number,
  totalAngleDeg: number,
  axisX: number, axisY: number, axisZ: number,
): Promise<void> {
  return invoke("update_circular_pattern", {
    featureId, count, totalAngleDeg, axisX, axisY, axisZ,
  });
}

/** Update the mirror plane of an existing Mirror feature.
 *  Document mutation — triggers snapshot + regen. */
export async function updateMirrorParams(
  featureId: FeatureId,
  planeNx: number, planeNy: number, planeNz: number,
  planePx: number, planePy: number, planePz: number,
): Promise<void> {
  return invoke("update_mirror_params", {
    featureId, planeNx, planeNy, planeNz, planePx, planePy, planePz,
  });
}
