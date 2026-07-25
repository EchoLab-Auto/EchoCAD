/**
 * EchoCAD Agent API — High-level modeling interface for AI agents.
 *
 * This module provides a clean, documented API for creating and
 * manipulating CAD models programmatically. All operations are
 * asynchronous and map to Tauri backend commands.
 *
 * ## Quick Start
 *
 * ```ts
 * import { AgentCAD } from "@/commands/agent-api";
 *
 * const cad = new AgentCAD();
 *
 * // Create a sketch
 * const sketchId = await cad.newSketch("xy");
 * await cad.line(0, 0, 10, 0);
 * await cad.line(10, 0, 10, 10);
 * await cad.line(10, 10, 0, 10);
 * await cad.line(0, 10, 0, 0);
 *
 * // Extrude it
 * await cad.extrude(sketchId, 5);
 *
 * // Capture a screenshot
 * const imageB64 = await cad.captureViewport();
 * ```
 *
 * ## Feature Coverage (API Parity — design principle #14)
 *
 * Every modeling step available in the UI is reachable through this API.
 *
 * | Category     | Methods |
 * |--------------|---------|
 * | Sketch       | newSketch, activateSketch, offsetPlane, line, circle, arc, rectangle, spline, ellipse, point, deleteEntity, clear, sketchEntities |
 * | Constraints  | coincident, horizontal, vertical, distanceConstraint, radius, parallel, perpendicular, tangent, concentric, fix, midpoint, symmetric, angle, removeConstraint, updateConstraint, solve, sketchConstraints |
 * | Extrude      | extrude, extrudeTwoSides, extrudeMidplane, getExtrudeRegions, previewExtrude (all accept `selectedRegions`) |
 * | Revolve      | revolve (with optional axis entity) |
 * | Sweep        | sweep |
 * | Fillet       | fillet, filletEdges |
 * | Chamfer      | chamfer, chamferEdges |
 * | Shell        | shell |
 * | Boolean      | union, subtract, intersect |
 * | Pattern      | linearPattern, circularPattern, mirror, editLinearPattern, editCircularPattern, editMirror |
 * | Feature edit | setDepth, setSuppressed, rename, setColor, delete, setEntityProp, movePoint |
 * | Query        | listFeatures, massProperties, measureDistance3D, measureAngle, regenErrors, allSolidMeshes |
 * | Plugins      | listPlugins, listGenerators, runGenerator |
 * | File         | save, saveTo, load, loadFrom, clearAll, exportStl, exportObj, exportGltf, hasRecoveryFile |
 * | System       | undo, redo, canUndoRedo, captureViewport, setBrep, isBrepEnabled |
 */

import {
  type FeatureId, type EntityId, type ParameterId,
  addLine, addCircle, addArc, addPoint, addSpline, addEllipse, deleteEntity,
  addConstraint, removeConstraint, solveSketch,
  addSketchFeature, setActiveSketch, getFeatures,
  addExtrudeFeature, addRevolveFeature, addSweepFeature,
  addFilletFeature, addFilletEdgesFeature, addChamferFeature, addChamferEdgesFeature,
  addShellFeature, addBooleanFeature, addMirrorFeature,
  addLinearPattern, addCircularPattern,
  deleteFeature, setFeatureSuppressed, updateParameter, renameFeature,
  createOffsetPlane, clearSketch, clearDocument,
  getMassProperties, measureDistance, measureAngle,
  saveProject, loadProject as loadProjectCmd, loadProjectFrom, saveProjectTo,
  getUseBrep, setUseBrep, captureViewport,
  undo, redo, canUndoRedo,
  getSketchEntities, getSketchConstraints, getRegenErrors,
  getExtrudeRegions, previewExtrude,
  updateLinearPattern, updateCircularPattern, updateMirrorParams,
  setFeatureColor, movePoint, updateEntityProp, updateConstraintValue,
  exportStl, exportObj, exportGltf, checkRecovery,
  listPlugins, listGenerators, generatePluginFeature,
  getAllSolidMeshes,
} from "./sketch";
import type {
  FeatureNode, SketchEntity, Constraint, ExtrudeRegionInfo,
  RenderMesh, PluginInfo, GeneratorInfo, SolidMeshEntry,
} from "./sketch";

export type { FeatureId, EntityId, ParameterId };

// ── Types ────────────────────────────────────────────────────────

export type PlaneName = "xy" | "yz" | "zx";

export interface Point3D { x: number; y: number; z: number; }

export interface MassProperties {
  volume: number;
  surfaceArea: number;
  centroid: [number, number, number];
}

export interface FeatureInfo {
  id: FeatureId;
  name: string;
  type: string;
  suppressed: boolean;
  errors?: string;
}

// ── Agent CAD class ──────────────────────────────────────────────

export class AgentCAD {
  private activeSketchId: FeatureId | null = null;

  // ═══════════════════════════════════════════════════════════════
  // Sketch & Document
  // ═══════════════════════════════════════════════════════════════

  /** Create a new sketch on the given plane. Returns the sketch FeatureId. */
  async newSketch(plane: PlaneName = "xy"): Promise<FeatureId> {
    const id = await addSketchFeature(plane) as FeatureId;
    await setActiveSketch(id);
    this.activeSketchId = id;
    return id;
  }

  /** Switch to an existing sketch for editing. */
  async activateSketch(id: FeatureId): Promise<void> {
    await setActiveSketch(id);
    this.activeSketchId = id;
  }

  /** Get the currently active sketch ID. */
  getActiveSketchId(): FeatureId | null { return this.activeSketchId; }

  /** Delete a feature by ID (cascading). */
  async delete(featureId: FeatureId): Promise<void> {
    await deleteFeature(featureId);
    if (this.activeSketchId === featureId) this.activeSketchId = null;
  }

  /** Create an offset plane from a base plane tag ("xy", "yz", or "zx"). */
  async offsetPlane(basePlane: PlaneName, distance: number): Promise<FeatureId> {
    return createOffsetPlane(basePlane, distance) as Promise<FeatureId>;
  }

  // ═══════════════════════════════════════════════════════════════
  // Drawing
  // ═══════════════════════════════════════════════════════════════

  /** Draw a line from (x1,y1) to (x2,y2). Returns the line entity ID. */
  async line(x1: number, y1: number, x2: number, y2: number): Promise<EntityId> {
    const startId = await addPoint(x1, y1);
    const endId = await addPoint(x2, y2);
    if (startId === null || endId === null) throw new Error("Failed to create line endpoints");
    const lineId = await addLine(startId, endId);
    if (lineId === null) throw new Error("Failed to create line");
    return lineId;
  }

  /** Draw a circle centered at (cx,cy) with given radius. */
  async circle(cx: number, cy: number, radius: number): Promise<EntityId> {
    const centerId = await addPoint(cx, cy);
    if (centerId === null) throw new Error("Failed to create circle center point");
    const circleId = await addCircle(centerId, radius);
    if (circleId === null) throw new Error("Failed to create circle");
    return circleId;
  }

  /** Draw an arc centered at (cx,cy) from startAngle to endAngle (degrees). */
  async arc(cx: number, cy: number, radius: number, startDeg: number, endDeg: number): Promise<EntityId> {
    const centerId = await addPoint(cx, cy);
    if (centerId === null) throw new Error("Failed to create arc center point");
    const arcId = await addArc(centerId, radius, startDeg, endDeg);
    if (arcId === null) throw new Error("Failed to create arc");
    return arcId;
  }

  /** Draw a point at (x,y). */
  async point(x: number, y: number): Promise<EntityId> {
    const id = await addPoint(x, y);
    if (id === null) throw new Error("Failed to create point");
    return id;
  }

  /** Draw a spline through the given control points. */
  async spline(points: [number, number][]): Promise<EntityId> {
    const ids: EntityId[] = [];
    for (const [x, y] of points) {
      const id = await addPoint(x, y);
      if (id === null) throw new Error("Failed to create spline control point");
      ids.push(id);
    }
    const splineId = await addSpline(ids);
    if (splineId === null) throw new Error("Failed to create spline");
    return splineId;
  }

  /** Draw an ellipse. majorRx/majorRy define the major axis endpoint relative to center. */
  async ellipse(cx: number, cy: number, majorRx: number, majorRy: number, ratio: number): Promise<EntityId> {
    const centerId = await addPoint(cx, cy);
    if (centerId === null) throw new Error("Failed to create ellipse center point");
    const majorEndId = await addPoint(cx + majorRx, cy + majorRy);
    if (majorEndId === null) throw new Error("Failed to create ellipse major axis point");
    const ellipseId = await addEllipse(centerId, majorEndId, ratio);
    if (ellipseId === null) throw new Error("Failed to create ellipse");
    return ellipseId;
  }

  /** Draw a rectangle from (x1,y1) to (x2,y2). Auto-adds 4 lines + constraints. */
  async rectangle(x1: number, y1: number, x2: number, y2: number): Promise<EntityId[]> {
    const p1 = await this.point(x1, y1);
    const p2 = await this.point(x2, y1);
    const p3 = await this.point(x2, y2);
    const p4 = await this.point(x1, y2);
    const l1 = await this.line(x1, y1, x2, y1);
    const l2 = await this.line(x2, y1, x2, y2);
    const l3 = await this.line(x2, y2, x1, y2);
    const l4 = await this.line(x1, y2, x1, y1);
    // Auto-constrain: horizontal top/bottom, vertical left/right
    await this.horizontal(l1);
    await this.horizontal(l3);
    await this.vertical(l2);
    await this.vertical(l4);
    return [p1, p2, p3, p4];
  }

  /** Delete an entity by ID. */
  async deleteEntity(id: EntityId): Promise<void> { await deleteEntity(id); }

  /** Clear all entities in the active sketch. */
  async clear(): Promise<void> { await clearSketch(); }

  // ═══════════════════════════════════════════════════════════════
  // Constraints
  // ═══════════════════════════════════════════════════════════════

  async coincident(a: EntityId, b: EntityId) { await addConstraint({ Coincident: { a, b } }); }
  async horizontal(lineId: EntityId) { await addConstraint({ Horizontal: { line: lineId } }); }
  async vertical(lineId: EntityId) { await addConstraint({ Vertical: { line: lineId } }); }
  async distanceConstraint(a: EntityId, b: EntityId, d: number) { await addConstraint({ Distance: { a, b, distance: d } }); }
  async radius(entity: EntityId, r: number) { await addConstraint({ Radius: { circle: entity, radius: r } }); }
  async parallel(a: EntityId, b: EntityId) { await addConstraint({ Parallel: { line_a: a, line_b: b } }); }
  async perpendicular(a: EntityId, b: EntityId) { await addConstraint({ Perpendicular: { line_a: a, line_b: b } }); }
  async tangent(lineId: EntityId, circleId: EntityId) { await addConstraint({ Tangent: { line: lineId, circle: circleId } }); }
  async concentric(a: EntityId, b: EntityId) { await addConstraint({ Concentric: { a, b } }); }
  async fix(pointId: EntityId) { await addConstraint({ Fix: { point: pointId } }); }
  async midpoint(pointId: EntityId, lineId: EntityId) { await addConstraint({ Midpoint: { point: pointId, line: lineId } }); }
  async symmetric(a: EntityId, b: EntityId, axisId: EntityId) { await addConstraint({ Symmetric: { a, b, axis: axisId } }); }
  async angle(a: EntityId, b: EntityId, angleDeg: number) { await addConstraint({ Angle: { line_a: a, line_b: b, angle_deg: angleDeg } }); }

  /** Remove a constraint by its index in the constraint list. */
  async removeConstraint(index: number): Promise<void> { await removeConstraint(index); }
  /** Solve the active sketch constraints. */
  async solve(): Promise<void> { await solveSketch(); }

  // ═══════════════════════════════════════════════════════════════
  // Solid Features
  // ═══════════════════════════════════════════════════════════════

  /** Extrude the given sketch by `depth` in OneSide mode.
   *  Pass `selectedRegions` to extrude only specific loops (see getExtrudeRegions). */
  async extrude(sketchId: FeatureId, depth: number, draftAngle = 0, selectedRegions?: number[]): Promise<FeatureId> {
    return addExtrudeFeature(sketchId, "one_side", 0, draftAngle, depth, selectedRegions ?? null) as Promise<FeatureId>;
  }

  /** List the closed loops in the active sketch available for extrusion.
   *  Indices returned here are what `extrude(..., selectedRegions)` expects. */
  async getExtrudeRegions(): Promise<ExtrudeRegionInfo[]> {
    return getExtrudeRegions();
  }

  /** Preview an extrusion without committing it to the document. */
  async previewExtrude(sketchId: FeatureId, depth: number, selectedRegions?: number[]): Promise<RenderMesh | null> {
    return previewExtrude(sketchId, "one_side", 0, 0, depth, selectedRegions ?? null);
  }

  /** Extrude in TwoSides mode with dist1 and dist2.
   *  addExtrudeFeature params: (sketchId, direction, dist2, draftAngleDeg, depth) */
  async extrudeTwoSides(sketchId: FeatureId, dist1: number, dist2: number, draftAngle = 0): Promise<FeatureId> {
    return addExtrudeFeature(sketchId, "two_sides", dist2, draftAngle, dist1) as Promise<FeatureId>;
  }

  /** Extrude symmetrically (Midplane).
   *  addExtrudeFeature params: (sketchId, direction, dist2, draftAngleDeg, depth) */
  async extrudeMidplane(sketchId: FeatureId, totalDepth: number): Promise<FeatureId> {
    return addExtrudeFeature(sketchId, "midplane", 0, 0, totalDepth) as Promise<FeatureId>;
  }

  /** Revolve a sketch around its default axis (Y axis in sketch plane).
   *  Pass axisEntityId to revolve around a specific sketch line. */
  async revolve(sketchId: FeatureId, axisEntityId?: EntityId): Promise<FeatureId> {
    return addRevolveFeature(sketchId, axisEntityId ?? null) as Promise<FeatureId>;
  }

  /** Sweep a profile sketch along a path sketch. */
  async sweep(profileId: FeatureId, pathId: FeatureId): Promise<FeatureId> {
    return addSweepFeature(profileId, pathId) as Promise<FeatureId>;
  }

  /** Apply a fillet to all sharp edges of a solid. */
  async fillet(targetId: FeatureId, radius: number): Promise<FeatureId> {
    return addFilletFeature(targetId, radius) as Promise<FeatureId>;
  }

  /** Apply a fillet to specific edges. Each edge is a (vertexA, vertexB) index pair
   *  into the target solid's mesh. An empty list falls back to all sharp edges. */
  async filletEdges(targetId: FeatureId, radius: number, edges: [number, number][]): Promise<FeatureId> {
    return addFilletEdgesFeature(targetId, radius, edges) as Promise<FeatureId>;
  }

  /** Apply a chamfer to all sharp edges of a solid. */
  async chamfer(targetId: FeatureId, distance: number): Promise<FeatureId> {
    return addChamferFeature(targetId, distance) as Promise<FeatureId>;
  }

  /** Apply a chamfer to specific edges. See {@link filletEdges} for the edge convention. */
  async chamferEdges(targetId: FeatureId, distance: number, edges: [number, number][]): Promise<FeatureId> {
    return addChamferEdgesFeature(targetId, distance, edges) as Promise<FeatureId>;
  }

  /** Hollow a solid with given wall thickness. */
  async shell(targetId: FeatureId, thickness: number): Promise<FeatureId> {
    return addShellFeature(targetId, thickness) as Promise<FeatureId>;
  }

  /** Boolean union of two solids. */
  async union(aId: FeatureId, bId: FeatureId): Promise<FeatureId> {
    return addBooleanFeature(aId, bId, "Union") as Promise<FeatureId>;
  }

  /** Boolean subtract (a - b). */
  async subtract(aId: FeatureId, bId: FeatureId): Promise<FeatureId> {
    return addBooleanFeature(aId, bId, "Subtract") as Promise<FeatureId>;
  }

  /** Boolean intersect. */
  async intersect(aId: FeatureId, bId: FeatureId): Promise<FeatureId> {
    return addBooleanFeature(aId, bId, "Intersect") as Promise<FeatureId>;
  }

  /** Create a linear pattern.
   *  addLinearPattern params: (targetId, dirX, dirY, dirZ, count, spacing) */
  async linearPattern(targetId: FeatureId, count: number, spacing: number, dirX = 1, dirY = 0, dirZ = 0): Promise<FeatureId> {
    return addLinearPattern(targetId, dirX, dirY, dirZ, count, spacing) as Promise<FeatureId>;
  }

  /** Create a circular pattern around the Z axis by default.
   *  addCircularPattern params: (targetId, axisX, axisY, axisZ, axisDx, axisDy, axisDz, count, totalAngleDeg) */
  async circularPattern(targetId: FeatureId, count: number, totalAngleDeg = 360): Promise<FeatureId> {
    return addCircularPattern(targetId, 0, 0, 0, 0, 0, 1, count, totalAngleDeg) as Promise<FeatureId>;
  }

  /** Mirror a solid across the XY plane. */
  async mirror(targetId: FeatureId): Promise<FeatureId> {
    return addMirrorFeature(targetId, 0, 0, 1, 0, 0, 0) as Promise<FeatureId>;
  }

  // ═══════════════════════════════════════════════════════════════
  // Edit
  // ═══════════════════════════════════════════════════════════════

  /** Change an extrude depth parameter. */
  async setDepth(featureId: FeatureId, depth: number): Promise<void> {
    const features = await getFeatures();
    const f = features.find(x => x.id === featureId);
    if (!f) throw new Error(`Feature ${featureId} not found`);
    const param = f.parameters.find(p => p.name !== "angle" && p.name !== "radius" && p.name !== "thickness");
    if (param) await updateParameter(param.id, depth);
  }

  /** Suppress or unsuppress a feature. */
  async setSuppressed(featureId: FeatureId, suppressed: boolean): Promise<void> {
    await setFeatureSuppressed(featureId, suppressed);
  }

  /** Rename a feature in the tree. */
  async rename(featureId: FeatureId, name: string): Promise<void> {
    await renameFeature(featureId, name);
  }

  /** Set (or clear, with null) a feature's display color as a hex string like "#ff8800". */
  async setColor(featureId: FeatureId, color: string | null): Promise<void> {
    await setFeatureColor(featureId, color);
  }

  /** Update a linear pattern's parameters in place. */
  async editLinearPattern(featureId: FeatureId, count: number, spacing: number, dirX = 1, dirY = 0, dirZ = 0): Promise<void> {
    await updateLinearPattern(featureId, count, spacing, dirX, dirY, dirZ);
  }

  /** Update a circular pattern's parameters in place (axis defaults to +Z through origin). */
  async editCircularPattern(featureId: FeatureId, count: number, totalAngleDeg = 360,
    axisX = 0, axisY = 0, axisZ = 0): Promise<void> {
    await updateCircularPattern(featureId, count, totalAngleDeg, axisX, axisY, axisZ);
  }

  /** Update a mirror feature's plane in place. */
  async editMirror(featureId: FeatureId,
    planeNx: number, planeNy: number, planeNz: number,
    planePx = 0, planePy = 0, planePz = 0): Promise<void> {
    await updateMirrorParams(featureId, planeNx, planeNy, planeNz, planePx, planePy, planePz);
  }

  /** Move a sketch point to new coordinates (re-solves afterwards). */
  async movePoint(pointId: EntityId, x: number, y: number): Promise<void> {
    await movePoint(pointId, x, y);
  }

  /** Change a numeric property of a sketch entity (e.g. "radius", "x", "y"). */
  async setEntityProp(entityId: EntityId, prop: string, value: number): Promise<boolean> {
    return updateEntityProp(entityId, prop, value);
  }

  /** Update a driving constraint's value in place (matches the existing constraint
   *  by kind + entity references and rewrites its value). */
  async updateConstraint(constraint: Constraint): Promise<boolean> {
    return updateConstraintValue(constraint);
  }

  async undo(): Promise<void> { await undo(); }
  async redo(): Promise<void> { await redo(); }
  /** Query undo/redo availability. */
  async canUndoRedo(): Promise<[boolean, boolean]> { return canUndoRedo(); }

  // ═══════════════════════════════════════════════════════════════
  // Query
  // ═══════════════════════════════════════════════════════════════

  /** List all features in the document. */
  async listFeatures(): Promise<FeatureInfo[]> {
    const features = await getFeatures();
    return features.map((f: FeatureNode) => ({
      id: f.id,
      name: f.name,
      type: f.feature_type,
      suppressed: f.suppressed,
      errors: f.errors ?? undefined,
    }));
  }

  /** Get mass properties for a solid feature. */
  async massProperties(featureId: FeatureId): Promise<MassProperties | null> {
    const p = await getMassProperties(featureId);
    if (!p) return null;
    return { volume: p.volume, surfaceArea: p.surface_area, centroid: p.centroid };
  }

  /** Measure 3D distance between two points. */
  async measureDistance3D(p1: Point3D, p2: Point3D): Promise<number> {
    return measureDistance(p1.x, p1.y, p1.z, p2.x, p2.y, p2.z);
  }

  /** Measure the angle <p1-p2-p3 in degrees. */
  async measureAngle(p1: Point3D, p2: Point3D, p3: Point3D): Promise<number> {
    return measureAngle(p1.x, p1.y, p1.z, p2.x, p2.y, p2.z, p3.x, p3.y, p3.z);
  }

  /** Read the entities of the active sketch (points/lines/circles…). */
  async sketchEntities(): Promise<SketchEntity[]> {
    return getSketchEntities();
  }

  /** Read the constraints of the active sketch. */
  async sketchConstraints(): Promise<Constraint[]> {
    return getSketchConstraints();
  }

  /** Per-feature regeneration errors (feature id + message), sorted by id. */
  async regenErrors(): Promise<[FeatureId, string][]> {
    return getRegenErrors();
  }

  /** All solid meshes currently produced by the feature tree. */
  async allSolidMeshes(): Promise<SolidMeshEntry[]> {
    return getAllSolidMeshes();
  }

  // ═══════════════════════════════════════════════════════════════
  // Plugins
  // ═══════════════════════════════════════════════════════════════

  /** List all registered plugins with their generators and tools. */
  async listPlugins(): Promise<PluginInfo[]> { return listPlugins(); }

  /** List all feature generators across all plugins. */
  async listGenerators(): Promise<GeneratorInfo[]> { return listGenerators(); }

  /** Run a plugin generator, creating a new feature in the document. */
  async runGenerator(pluginId: string, generatorId: string, params: Record<string, number>): Promise<FeatureId> {
    return generatePluginFeature(pluginId, generatorId, params) as Promise<FeatureId>;
  }

  // ═══════════════════════════════════════════════════════════════
  // Viewport & File
  // ═══════════════════════════════════════════════════════════════

  /** Capture the 3D viewport as a base64-encoded PNG image. */
  async captureViewport(): Promise<string> {
    return captureViewport();
  }

  /** Toggle B-rep pipeline (OCCT precise geometry). */
  async setBrep(enabled: boolean): Promise<void> { await setUseBrep(enabled); }
  /** Query whether B-rep pipeline is active. */
  async isBrepEnabled(): Promise<boolean> { return getUseBrep(); }

  /** Save the current project. */
  async save(): Promise<string> { return saveProject(); }
  /** Save the current project to a specific path. */
  async saveTo(path: string): Promise<void> { await saveProjectTo(path); }
  /** Load a project via file dialog. */
  async load(): Promise<void> { await loadProjectCmd(); }
  /** Load a project from a specific path. */
  async loadFrom(path: string): Promise<void> { await loadProjectFrom(path); }
  /** Clear the entire document (new empty part). */
  async clearAll(): Promise<void> { await clearDocument(); }

  /** Export the merged solid as STL. Returns the file path. */
  async exportStl(): Promise<string> { return exportStl(); }
  /** Export the merged solid as OBJ. Returns the file path. */
  async exportObj(): Promise<string> { return exportObj(); }
  /** Export the merged solid as glTF 2.0. Returns the file path. */
  async exportGltf(): Promise<string> { return exportGltf(); }

  /** Check whether a crash-recovery autosave exists. */
  async hasRecoveryFile(): Promise<boolean> { return checkRecovery(); }
}

/** Convenience: create a single global agent instance. */
export const cad = new AgentCAD();
