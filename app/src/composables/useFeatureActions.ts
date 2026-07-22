import { ref, type Ref } from "vue";
import { useSketchStore } from "@/stores/sketch";
import { useToastStore } from "@/stores/toast";
import {
  getFeatures, getSketchEntities,
  previewExtrude, addExtrudeFeature,
  addRevolveFeature,
  addFilletEdgesFeature, addChamferEdgesFeature,
  addLinearPattern as addLinearPatternCmd,
  addCircularPattern as addCircularPatternCmd,
  addMirrorFeature, addSweepFeature, addShellFeature,
  addBooleanFeature as addBooleanFeatureCmd,
} from "@/commands/sketch";
import type { FeatureId, FeatureNode } from "@/commands/sketch";
import type { FeatureKind, PatternKind } from "@/components/Toolbar.vue";
import type { BooleanTargets, BooleanResult } from "@/components/BooleanDialog.vue";
import type UnifiedViewport from "@/components/UnifiedViewport.vue";

/**
 * Feature-creation actions extracted from HomeView.vue.
 *
 * Takes the UnifiedViewport template ref so it can call refreshSketch /
 * refreshViewport / showPreviewMesh / clearPreviewMesh after every mutation.
 */
export function useFeatureActions(
  unifiedViewport: Ref<InstanceType<typeof UnifiedViewport> | null>,
) {
  const sketchStore = useSketchStore();
  const toastStore = useToastStore();

  let extrudePreviewTimer: ReturnType<typeof setTimeout> | null = null;
  const booleanDialog = ref<BooleanTargets | null>(null);

  // ── State helpers ─────────────────────────────────────────────────

  async function loadState() {
    sketchStore.isLoading = true;
    try {
      sketchStore.setFeatures(await getFeatures());
      sketchStore.setEntities(await getSketchEntities());
      unifiedViewport.value?.refreshSketch();
    } finally {
      sketchStore.isLoading = false;
    }
  }

  /// True for any feature that produces a solid body and therefore makes a
  /// valid fillet/chamfer/shell/boolean target. Includes pattern/mirror/boolean
  /// results and plugin-generated solids.
  function isSolidFeature(f: FeatureNode): boolean {
    return !f.suppressed && (
      ["Extrude", "Revolve", "Fillet", "Chamfer", "LinearPattern", "CircularPattern",
        "Mirror", "Sweep", "Shell", "Boolean"].includes(f.feature_type) ||
      f.feature_type.startsWith("CustomSolid")
    );
  }

  /// The currently-selected solid, if any. Face picks in the viewport set
  /// `selectedFeatureId` to the clicked body, so this naturally reflects a
  /// face click as well as a feature-tree click.
  function selectedSolidFeatureId(): FeatureId | null {
    const sel = sketchStore.selectedFeatureId;
    if (sel === null) return null;
    const f = sketchStore.features.find(x => x.id === sel);
    return f && isSolidFeature(f) ? f.id : null;
  }

  function lastSolidFeatureId(): FeatureId | null {
    const solids = sketchStore.features.filter(isSolidFeature);
    return solids.length > 0 ? solids[solids.length - 1].id : null;
  }

  /**
   * The sketch to use for feature operations (extrude / revolve / sweep source).
   * Prefers the sketch being edited; falls back to the selected feature if
   * it's a sketch. Returns null when nothing usable is selected.
   */
  function activeSketchForFeature(): FeatureId | null {
    if (sketchStore.editingSketchId !== null) return sketchStore.editingSketchId;
    const f = sketchStore.features.find(x => x.id === sketchStore.activeFeatureId);
    if (f && (f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"))) {
      return f.id;
    }
    return null;
  }

  // ── Shared action wrapper ─────────────────────────────────────────

  /// Wrap a feature-creation call: shows toast on error, refreshes state on success.
  async function withFeatureAction<T>(label: string, fn: () => Promise<T>): Promise<T | null> {
    try {
      const result = await fn();
      await loadState();
      await unifiedViewport.value?.refreshViewport();
      toastStore.success(`${label}已创建`);
      return result;
    } catch (err) {
      toastStore.error(`${label}失败: ${err}`);
      return null;
    }
  }

  // ── Extrude ──────────────────────────────────────────────────────

  async function showExtrudePanel() {
    if (activeSketchForFeature() === null) {
      toastStore.info("请先选择一个草图");
      return;
    }
    sketchStore.showExtrudePanel = true;
    updateExtrudePreview(sketchStore.extrudeConfig);
  }

  async function onExtrudeConfigChange(config: { direction: string; depth: number; dist2: number; draft: number }) {
    sketchStore.extrudeConfig = { ...config };
    if (extrudePreviewTimer) clearTimeout(extrudePreviewTimer);
    extrudePreviewTimer = setTimeout(() => updateExtrudePreview(config), 120);
  }

  async function updateExtrudePreview(cfg: { direction: string; depth: number; dist2: number; draft: number }) {
    const activeId = activeSketchForFeature();
    if (activeId === null) return;
    const mesh = await previewExtrude(activeId, cfg.direction, cfg.dist2, cfg.draft, cfg.depth);
    if (mesh) {
      unifiedViewport.value?.showPreviewMesh(mesh);
    }
  }

  function cancelExtrude() {
    if (extrudePreviewTimer) clearTimeout(extrudePreviewTimer);
    sketchStore.showExtrudePanel = false;
    unifiedViewport.value?.clearPreviewMesh();
  }

  async function doExtrude() {
    const activeId = activeSketchForFeature();
    if (activeId === null) return;
    const cfg = sketchStore.extrudeConfig;
    if (extrudePreviewTimer) clearTimeout(extrudePreviewTimer);
    sketchStore.showExtrudePanel = false;
    unifiedViewport.value?.clearPreviewMesh();
    try {
      await addExtrudeFeature(activeId, cfg.direction, cfg.dist2, cfg.draft, cfg.depth);
      await loadState();
      await unifiedViewport.value?.refreshViewport();
      toastStore.success("拉伸特征已创建");
    } catch (err) {
      toastStore.error(`拉伸失败: ${err}`);
    }
  }

  // ── Other features ───────────────────────────────────────────────

  async function revolve() {
    const activeId = activeSketchForFeature();
    if (activeId === null) {
      toastStore.info("请先选择一个草图");
      return;
    }
    const axisId = sketchStore.selectedId ?? null;
    await withFeatureAction("旋转特征", () => addRevolveFeature(activeId, axisId));
  }

  async function addFillet() {
    const tid = selectedSolidFeatureId();
    if (tid === null) {
      toastStore.info("请先点击一个实体面（或选中实体特征）");
      return;
    }
    const id = await withFeatureAction("圆角特征", () => addFilletEdgesFeature(tid, 0.5, []));
    if (id !== null) sketchStore.selectedFeatureId = id;
  }

  async function addChamfer() {
    const tid = selectedSolidFeatureId();
    if (tid === null) {
      toastStore.info("请先点击一个实体面（或选中实体特征）");
      return;
    }
    const id = await withFeatureAction("倒角特征", () => addChamferEdgesFeature(tid, 0.5, []));
    if (id !== null) sketchStore.selectedFeatureId = id;
  }

  async function addLinearPattern() {
    const tid = lastSolidFeatureId();
    if (tid === null) {
      toastStore.info("请先创建一个实体");
      return;
    }
    await withFeatureAction("线性阵列", () => addLinearPatternCmd(tid, 1.0, 0.0, 0.0, 3, 2.0));
  }

  async function addCircularPattern() {
    const tid = lastSolidFeatureId();
    if (tid === null) {
      toastStore.info("请先创建一个实体");
      return;
    }
    await withFeatureAction("圆周阵列", () => addCircularPatternCmd(tid, 0, 0, 0, 0, 0, 1, 4, 360));
  }

  async function addMirror() {
    const tid = lastSolidFeatureId();
    if (tid === null) {
      toastStore.info("请先创建一个实体");
      return;
    }
    await withFeatureAction("镜像特征", () => addMirrorFeature(tid, 1, 0, 0, 0, 0, 0));
  }

  async function addSweep() {
    const activeId = activeSketchForFeature();
    if (activeId === null) {
      toastStore.info("请先选择一个草图");
      return;
    }
    const otherSketches = sketchStore.features.filter(f => f.feature_type === "Sketch" && f.id !== activeId);
    if (otherSketches.length === 0) {
      toastStore.info("需要两个草图：轮廓 + 路径");
      return;
    }
    await withFeatureAction("扫描特征", () => addSweepFeature(activeId, otherSketches[0].id));
  }

  async function addShell() {
    const tid = selectedSolidFeatureId();
    if (tid === null) {
      toastStore.info("请先点击一个实体面（或选中实体特征）");
      return;
    }
    const id = await withFeatureAction("抽壳特征", () => addShellFeature(tid, 0.5));
    if (id !== null) sketchStore.selectedFeatureId = id;
  }

  // ── Boolean ──────────────────────────────────────────────────────

  /// Open the boolean-op dialog. Target A defaults to the selected solid
  /// (so a face pick drives subtract/intersect order predictably); target B
  /// is the most recent *other* solid. If nothing is selected, A/B fall back
  /// to the two most recent solids in creation order. The dialog lets the
  /// user pick union/subtract/intersect and swap A↔B before confirming.
  async function addBoolean() {
    const solids = sketchStore.features.filter(isSolidFeature);
    if (solids.length < 2) {
      toastStore.info("布尔运算需要两个实体");
      return;
    }
    const sel = sketchStore.selectedFeatureId;
    let a: FeatureNode;
    let b: FeatureNode;
    const selFeature = sel !== null ? solids.find(f => f.id === sel) : undefined;
    if (selFeature) {
      a = selFeature;
      const others = solids.filter(f => f.id !== sel);
      b = others[others.length - 1] ?? solids[solids.length - 1];
    } else {
      a = solids[solids.length - 1];
      b = solids[solids.length - 2];
    }
    booleanDialog.value = { idA: a.id, nameA: a.name, idB: b.id, nameB: b.name };
  }

  async function confirmBoolean(result: BooleanResult) {
    const { op, idA, idB } = result;
    booleanDialog.value = null;
    const id = await withFeatureAction("布尔特征", () => addBooleanFeatureCmd(idA, idB, op));
    if (id !== null) sketchStore.selectedFeatureId = id;
  }

  function cancelBoolean() {
    booleanDialog.value = null;
  }

  // ── Toolbar action dispatchers ───────────────────────────────────

  function onFeatureAction(kind: FeatureKind) {
    switch (kind) {
      case "extrude": showExtrudePanel(); break;
      case "revolve": revolve(); break;
      case "sweep": addSweep(); break;
      case "fillet": addFillet(); break;
      case "chamfer": addChamfer(); break;
      case "shell": addShell(); break;
      case "boolean": addBoolean(); break;
    }
  }

  function onPatternAction(kind: PatternKind) {
    switch (kind) {
      case "linear": addLinearPattern(); break;
      case "circular": addCircularPattern(); break;
      case "mirror": addMirror(); break;
    }
  }

  return {
    // extrude
    doExtrude,
    cancelExtrude,
    onExtrudeConfigChange,
    showExtrudePanel,
    // feature creation
    revolve,
    addFillet,
    addChamfer,
    addShell,
    addLinearPattern,
    addCircularPattern,
    addMirror,
    addSweep,
    addBoolean,
    // boolean dialog
    booleanDialog,
    confirmBoolean,
    cancelBoolean,
    // dispatchers
    onFeatureAction,
    onPatternAction,
  };
}
