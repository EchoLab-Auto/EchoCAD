import { ref, computed, nextTick, type Ref } from "vue";
import { useSketchStore } from "@/stores/sketch";
import { useToastStore } from "@/stores/toast";
import {
  getFeatures, getSketchEntities,
  addConstraint, solveSketch,
  updateEntityProp as updateEntityPropCmd,
  deleteEntity as deleteEntityCmd,
  removeConstraint,
  setActiveSketch, addSketchFeature,
  deleteFeature as deleteFeatureCmd,
  renameFeature as renameFeatureCmd,
  setFeatureSuppressed as setFeatureSuppressedCmd,
  clearSketch,
} from "@/commands/sketch";
import type { EntityId, FeatureId, FeatureNode } from "@/commands/sketch";
import type { ConstraintKind } from "@/components/Toolbar.vue";
import type { CtxTarget } from "@/components/ContextMenu.vue";
import type { CascadeTarget } from "@/components/CascadeDeleteDialog.vue";
import type UnifiedViewport from "@/components/UnifiedViewport.vue";
import type Toolbar from "@/components/Toolbar.vue";

/**
 * Sketch-editing, constraint, entity-manipulation, and feature-tree actions
 * extracted from HomeView.vue.
 *
 * Takes template refs for UnifiedViewport and Toolbar so it can call
 * refreshSketch / refreshViewport / closeAllMenus after every mutation.
 */
export function useSketchActions(
  unifiedViewport: Ref<InstanceType<typeof UnifiedViewport> | null>,
  toolbarRef: Ref<InstanceType<typeof Toolbar> | null>,
) {
  const sketchStore = useSketchStore();
  const toastStore = useToastStore();

  const cascadeDialog = ref<CascadeTarget | null>(null);
  const ctxMenu = ref<CtxTarget | null>(null);

  // ── State helpers ─────────────────────────────────────────────────

  async function loadState() {
    sketchStore.isLoading = true;
    try {
      sketchStore.setFeatures(await getFeatures());
      await refreshSketch();
    } finally {
      sketchStore.isLoading = false;
    }
  }

  async function refreshFeatures() {
    sketchStore.setFeatures(await getFeatures());
  }

  /// Sync sketch entities + constraints from the backend exactly once.
  /// The viewport's refreshSketch does the fetches AND re-renders; falling
  /// back to a bare fetch only before the viewport is mounted. Calling this
  /// composable's loadState afterwards must NOT re-fetch entities again —
  /// a constraint click previously cost ~5 getSketchEntities round-trips.
  async function refreshSketch() {
    if (unifiedViewport.value) {
      await unifiedViewport.value.refreshSketch();
    } else {
      sketchStore.setEntities(await getSketchEntities());
    }
  }

  // ── Entity pickers (used by constraint helpers) ──────────────────

  function pickTwoPoints(): [EntityId, EntityId] | null {
    const points = sketchStore.entities.filter(e => e.type === "Point");
    if (points.length < 2) return null;
    const sel = sketchStore.selectedId;
    if (sel !== null && points.some(p => p.id === sel)) {
      const other = points.find(p => p.id !== sel);
      if (other) return [sel, other.id];
    }
    // Don't fall back to arbitrary entities — user must select
    return null;
  }

  function pickTwoLines(): [EntityId, EntityId] | null {
    const lines = sketchStore.entities.filter(e => e.type === "Line");
    if (lines.length < 2) return null;
    const sel = sketchStore.selectedId;
    if (sel !== null && lines.some(l => l.id === sel)) {
      const other = lines.find(l => l.id !== sel);
      if (other) return [sel, other.id];
    }
    return null;
  }

  function pickLineAndCircle(): [EntityId, EntityId] | null {
    const lines = sketchStore.entities.filter(e => e.type === "Line");
    const circles = sketchStore.entities.filter(e => e.type === "Circle" || e.type === "Arc");
    if (lines.length === 0 || circles.length === 0) return null;
    const sel = sketchStore.selectedId;
    if (sel !== null && lines.some(l => l.id === sel)) return [sel, circles[0].id];
    if (sel !== null && circles.some(c => c.id === sel)) return [lines[0].id, sel];
    return null;
  }

  function pickTwoCircles(): [EntityId, EntityId] | null {
    const circles = sketchStore.entities.filter(e => e.type === "Circle" || e.type === "Arc");
    if (circles.length < 2) return null;
    const sel = sketchStore.selectedId;
    if (sel !== null && circles.some(c => c.id === sel)) {
      const other = circles.find(c => c.id !== sel);
      if (other) return [sel, other.id];
    }
    return null;
  }

  function pickPointAndLine(): [EntityId, EntityId] | null {
    const points = sketchStore.entities.filter(e => e.type === "Point");
    const lines = sketchStore.entities.filter(e => e.type === "Line");
    if (points.length === 0 || lines.length === 0) return null;
    const sel = sketchStore.selectedId;
    if (sel !== null && points.some(p => p.id === sel)) return [sel, lines[0].id];
    if (sel !== null && lines.some(l => l.id === sel)) return [points[0].id, sel];
    return null;
  }

  // ── Constraint helpers ───────────────────────────────────────────

  async function addHorizontal() {
    const id = sketchStore.selectedId;
    if (id === null) {
      toastStore.info("请先选择一条线");
      return;
    }
    try {
      await addConstraint({ Horizontal: { line: id } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`水平约束添加失败: ${err}`);
    }
  }

  async function addVertical() {
    const id = sketchStore.selectedId;
    if (id === null) {
      toastStore.info("请先选择一条线");
      return;
    }
    try {
      await addConstraint({ Vertical: { line: id } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`垂直约束添加失败: ${err}`);
    }
  }

  async function addParallel() {
    const ids = pickTwoLines();
    if (!ids) { toastStore.info("需要两条线"); return; }
    try {
      await addConstraint({ Parallel: { line_a: ids[0], line_b: ids[1] } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`平行约束添加失败: ${err}`);
    }
  }

  async function addPerpendicular() {
    const ids = pickTwoLines();
    if (!ids) { toastStore.info("需要两条线"); return; }
    try {
      await addConstraint({ Perpendicular: { line_a: ids[0], line_b: ids[1] } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`正交约束添加失败: ${err}`);
    }
  }

  async function addTangent() {
    const ids = pickLineAndCircle();
    if (!ids) { toastStore.info("需要一条线和一个圆/弧"); return; }
    try {
      await addConstraint({ Tangent: { line: ids[0], circle: ids[1] } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`相切约束添加失败: ${err}`);
    }
  }

  async function addConcentric() {
    const ids = pickTwoCircles();
    if (!ids) { toastStore.info("需要两个圆/弧"); return; }
    try {
      await addConstraint({ Concentric: { a: ids[0], b: ids[1] } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`同心约束添加失败: ${err}`);
    }
  }

  async function addEqual() {
    const ids = pickTwoLines() ?? pickTwoCircles();
    if (!ids) { toastStore.info("需要两个同类实体"); return; }
    try {
      await addConstraint({ Equal: { a: ids[0], b: ids[1] } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`等长约束添加失败: ${err}`);
    }
  }

  async function addMidpoint() {
    const ids = pickPointAndLine();
    if (!ids) { toastStore.info("需要一个点 + 一条线"); return; }
    try {
      await addConstraint({ Midpoint: { point: ids[0], line: ids[1] } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`中点约束添加失败: ${err}`);
    }
  }

  async function addFixPoint() {
    const id = sketchStore.selectedId;
    if (id === null) {
      toastStore.info("请先选择一个点");
      return;
    }
    try {
      await addConstraint({ Fix: { point: id } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`固定约束添加失败: ${err}`);
    }
  }

  async function addAngle() {
    const ids = pickTwoLines();
    if (!ids) { toastStore.info("需要两条线"); return; }
    try {
      await addConstraint({ Angle: { line_a: ids[0], line_b: ids[1], angle_deg: 90.0 } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`角度约束添加失败: ${err}`);
    }
  }

  async function addDiameter() {
    const circles = sketchStore.entities.filter(e => e.type === "Circle" || e.type === "Arc");
    if (circles.length === 0) { toastStore.info("需要圆/弧"); return; }
    const sel = sketchStore.selectedId;
    const target = (sel !== null && circles.some(c => c.id === sel))
      ? circles.find(c => c.id === sel)!
      : circles[0];
    const r = target.type === "Circle" ? target.radius : (target as any).radius;
    try {
      await addConstraint({ Diameter: { circle: target.id, diameter: r * 2 } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`直径约束添加失败: ${err}`);
    }
  }

  async function addDistance() {
    const ids = pickTwoPoints();
    if (!ids) { toastStore.info("需要两个点"); return; }
    try {
      await addConstraint({ Distance: { a: ids[0], b: ids[1], distance: 2.0 } });
      await solveSketch();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`距离约束添加失败: ${err}`);
    }
  }

  async function solve() {
    const diagnostics = await solveSketch();
    if (diagnostics.length > 0) {
      // Show first diagnostic as a warning; additional ones as info
      for (const msg of diagnostics) {
        if (msg.includes("over-constrained") || msg.includes("conflict")) {
          toastStore.error(msg);
        } else {
          toastStore.info(msg);
        }
      }
    }
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }

  /// Delete a single constraint by its index in store.constraints. The
  /// `removeConstraint` Tauri command takes an index (not an id) — the
  /// PropertiesPanel passes the index from its v-for, which matches the
  /// backend's current constraint ordering. After the deletion we re-sync
  /// the store (read-your-write, design-principle #6.1) so the list and
  /// the dimension/glyph overlay both update.
  async function deleteConstraint(index: number) {
    try {
      await removeConstraint(index);
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`约束删除失败: ${err}`);
    }
  }

  /// Clear every constraint on the active sketch. Repeatedly remove index 0
  /// (each removal shifts later indices down) until none remain. A single
  /// refreshSketch at the end is enough since the backend mutation is
  /// synchronous w.r.t. our await chain.
  async function clearAllConstraints() {
    const n = sketchStore.constraints.length;
    try {
      for (let i = 0; i < n; i++) {
        await removeConstraint(0);
      }
      await loadState();
      await unifiedViewport.value?.refreshViewport();
    } catch (err) {
      toastStore.error(`约束清除失败: ${err}`);
    }
  }

  function onConstraintAction(kind: ConstraintKind) {
    switch (kind) {
      case "horizontal": addHorizontal(); break;
      case "vertical": addVertical(); break;
      case "parallel": addParallel(); break;
      case "perpendicular": addPerpendicular(); break;
      case "tangent": addTangent(); break;
      case "concentric": addConcentric(); break;
      case "equal": addEqual(); break;
      case "midpoint": addMidpoint(); break;
      case "fix": addFixPoint(); break;
      case "angle": addAngle(); break;
      case "diameter": addDiameter(); break;
      case "distance": addDistance(); break;
      case "solve": solve(); break;
    }
  }

  // ── Entity editing ───────────────────────────────────────────────

  async function updateEntityProp(id: EntityId, prop: string, event: Event) {
    const value = parseFloat((event.target as HTMLInputElement).value);
    if (Number.isNaN(value)) return;
    await updateEntityPropCmd(id, prop, value);
    await solveSketch();
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }

  async function deleteSelectedEntity() {
    const id = sketchStore.selectedId;
    if (id === null) return;
    await deleteEntityCmd(id);
    sketchStore.select(null);
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }

  async function toggleConstruction(id: EntityId, event: Event) {
    const checked = (event.target as HTMLInputElement).checked;
    await updateEntityPropCmd(id, "construction", checked ? 1 : 0);
    await refreshSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }

  // ── Feature tree navigation ──────────────────────────────────────

  /**
   * Left-click selection: highlight in the tree and show properties.
   * Does NOT enter sketch edit mode — that's only via double-click or the
   * right-click "编辑草图" menu (SolidWorks behavior).
   */
  async function selectFeature(id: FeatureId) {
    sketchStore.selectedFeatureId = id;
    sketchStore.setActiveFeature(id);
    // If we're editing a different sketch, switching selection exits edit mode.
    if (sketchStore.editingSketchId !== null && sketchStore.editingSketchId !== id) {
      await exitSketchEdit();
    }
    const f = sketchStore.features.find(x => x.id === id);
    // Sync the working plane for display (face view) even when not editing.
    if (f?.plane) {
      sketchStore.activePlane = f.plane;
      // Also store offset plane metadata for planeFrame resolution.
      if (f.plane === "offset") {
        sketchStore.activePlaneBase = f.plane_base ?? null;
        sketchStore.activePlaneDistance = f.plane_distance ?? null;
      }
    }
  }

  /**
   * Enter sketch edit mode for the given sketch feature. Shows sketch
   * entities in the viewport, enables drawing tools, and faces the plane.
   */
  async function enterSketchEdit(id: FeatureId) {
    const f = sketchStore.features.find(x => x.id === id);
    if (!f || !(f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"))) return;

    sketchStore.selectedFeatureId = id;
    sketchStore.setActiveFeature(id);
    sketchStore.setEditingSketch(id);
    sketchStore.setTool("select");

    if (f.plane) {
      sketchStore.activePlane = f.plane;
      if (f.plane === "offset") {
        sketchStore.activePlaneBase = f.plane_base ?? null;
        sketchStore.activePlaneDistance = f.plane_distance ?? null;
      }
    }
    if (sketchStore.showExtrudePanel) {
      sketchStore.showExtrudePanel = false;
      unifiedViewport.value?.clearPreviewMesh();
    }
    await setActiveSketch(id);
    await refreshSketch();
  }

  /** Double-click on a sketch enters edit mode; on other features, just selects. */
  function editFeature(feature: FeatureNode) {
    if (feature.feature_type === "Sketch" || feature.feature_type.startsWith("Custom:")) {
      enterSketchEdit(feature.id);
    } else {
      selectFeature(feature.id);
    }
  }

  async function newSketch() {
    const id = await addSketchFeature(sketchStore.activePlane);
    await refreshFeatures();
    // Creating a new sketch enters edit mode immediately (SolidWorks behavior).
    await enterSketchEdit(id);
    // A new sketch feature also mutates the document — refresh the viewport
    // to keep the sync invariant airtight (原则2), even though an empty
    // sketch currently produces no solid.
    await unifiedViewport.value?.refreshViewport();
  }

  async function clearSketchAction() {
    if (sketchStore.editingSketchId === null) return;
    await clearSketch();
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }

  async function exitSketchEdit() {
    if (sketchStore.editingSketchId === null) return;
    sketchStore.setEditingSketch(null);
    // Clear stale entities so they don't render on the wrong plane
    sketchStore.setEntities([]);
    toolbarRef.value?.closeAllMenus();
    await unifiedViewport.value?.refreshViewport();
  }

  // ── Feature deletion ─────────────────────────────────────────────

  async function deleteFeature(id: FeatureId) {
    const feature = sketchStore.features.find(f => f.id === id);
    if (!feature) return;

    // Try non-cascade first; the backend returns Err("dependents:name1,name2")
    // when there are dependents.
    try {
      await deleteFeatureCmd(id, false);
      if (sketchStore.selectedFeature?.id === id) sketchStore.selectedFeatureId = null;
      await loadState();
      await unifiedViewport.value?.refreshViewport();
      toastStore.success(`已删除 "${feature.name}"`);
    } catch (err: any) {
      // Parse dependent names from the structured error (原则 §7).
      const msg = String(err);
      let dependentNames: string[] = [];
      try {
        const parsed = JSON.parse(msg);
        if (parsed.kind === "dependents" && Array.isArray(parsed.names)) {
          dependentNames = parsed.names;
        }
      } catch {
        // Fallback to legacy string format for backward compatibility.
        if (msg.startsWith("dependents:")) {
          dependentNames = msg.slice("dependents:".length).split(",").filter(Boolean);
        }
      }
      cascadeDialog.value = { id, name: feature.name, dependents: dependentNames };
    }
  }

  async function confirmCascadeDelete() {
    if (!cascadeDialog.value) return;
    const { id, name } = cascadeDialog.value;
    cascadeDialog.value = null;
    try {
      await deleteFeatureCmd(id, true);
      if (sketchStore.selectedFeature?.id === id) sketchStore.selectedFeatureId = null;
      await loadState();
      await unifiedViewport.value?.refreshViewport();
      toastStore.success(`已级联删除 "${name}" 及其依赖项`);
    } catch (err: any) {
      toastStore.error(`删除失败: ${err}`);
    }
  }

  function cancelCascade() {
    cascadeDialog.value = null;
  }

  // ── Feature metadata ─────────────────────────────────────────────

  async function toggleSuppress(feature: FeatureNode) {
    await setFeatureSuppressedCmd(feature.id, !feature.suppressed);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }

  async function renameSelectedFeature(name: string) {
    const sel = sketchStore.selectedFeature;
    if (!sel) return;
    if (!name || name === sel.name) return;
    await renameFeatureCmd(sel.id, name);
    await loadState();
    await unifiedViewport.value?.refreshViewport();
  }

  // ── Context menu ─────────────────────────────────────────────────

  function onFeatureContextMenu(event: MouseEvent, featureId: FeatureId) {
    event.preventDefault();
    ctxMenu.value = { x: event.clientX, y: event.clientY, featureId };
  }

  function closeCtxMenu() { ctxMenu.value = null; }

  const ctxFeature = computed(() =>
    ctxMenu.value ? sketchStore.features.find(f => f.id === ctxMenu.value!.featureId) : null
  );
  const ctxFeatureSuppressed = computed(() => ctxFeature.value?.suppressed ?? false);

  async function ctxRename() {
    if (!ctxMenu.value) return;
    const f = sketchStore.features.find(x => x.id === ctxMenu.value!.featureId);
    if (!f) return;
    const n = prompt("重命名:", f.name);
    if (n && n !== f.name) {
      try {
        await renameFeatureCmd(f.id, n);
        await loadState();
        await unifiedViewport.value?.refreshViewport();
      } catch (err) {
        toastStore.error(`重命名失败: ${err}`);
      }
    }
    closeCtxMenu();
  }

  async function ctxDelete() {
    if (!ctxMenu.value) return;
    const fid = ctxMenu.value.featureId;
    closeCtxMenu();
    await deleteFeature(fid);
  }

  function ctxEditSketch() {
    if (!ctxMenu.value) return;
    const fid = ctxMenu.value.featureId;
    const f = sketchStore.features.find(x => x.id === fid);
    if (f && (f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"))) {
      enterSketchEdit(fid);
    }
    closeCtxMenu();
  }

  async function ctxToggleSuppress() {
    if (!ctxMenu.value || !ctxFeature.value) return;
    await toggleSuppress(ctxFeature.value);
    closeCtxMenu();
  }

  function ctxFaceNormal() {
    if (!ctxMenu.value) return;
    const fid = ctxMenu.value.featureId;
    selectFeature(fid);
    nextTick(() => { unifiedViewport.value?.setView("face"); });
    closeCtxMenu();
  }

  return {
    // constraints
    addHorizontal,
    addVertical,
    addParallel,
    addPerpendicular,
    addTangent,
    addConcentric,
    addEqual,
    addMidpoint,
    addFixPoint,
    addAngle,
    addDiameter,
    addDistance,
    onConstraintAction,
    solve,
    deleteConstraint,
    clearAllConstraints,
    // entity editing
    updateEntityProp,
    deleteSelectedEntity,
    toggleConstruction,
    // feature tree
    selectFeature,
    editFeature,
    enterSketchEdit,
    exitSketchEdit,
    newSketch,
    clearSketchAction,
    // feature deletion
    deleteFeature,
    confirmCascadeDelete,
    cancelCascade,
    cascadeDialog,
    // feature metadata
    toggleSuppress,
    renameSelectedFeature,
    // context menu
    onFeatureContextMenu,
    closeCtxMenu,
    ctxMenu,
    ctxFeatureSuppressed,
    ctxRename,
    ctxDelete,
    ctxEditSketch,
    ctxToggleSuppress,
    ctxFaceNormal,
  };
}
