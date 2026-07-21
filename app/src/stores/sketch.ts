import { defineStore } from "pinia";
import { ref, computed } from "vue";
import type {
  Constraint,
  EntityId,
  FeatureId,
  FeatureNode,
  SketchEntity,
  GeneratorInfo,
  PluginInfo,
} from "@/commands/sketch";

export type Tool = "select" | "line" | "circle" | "arc" | "rectangle" | "spline" | "ellipse" | "plugin";

export interface ExtrudeConfig {
  direction: string;
  depth: number;
  dist2: number;
  draft: number;
}

export const useSketchStore = defineStore("sketch", () => {
  const entities = ref<SketchEntity[]>([]);
  const constraints = ref<Constraint[]>([]);
  const features = ref<FeatureNode[]>([]);

  /// Per-feature color overrides keyed by FeatureId → hex color string.
  /// If a feature has no entry here, the renderer falls back to the default
  /// SOLID_COLORS palette. Currently UI-only (no backend command yet).
  const featureColors = ref<Record<number, string>>({});

  /// The "active" feature — what feature operations (extrude, revolve, ...)
  /// will target. Set on every tree selection. For sketches, this is also
  /// the sketch that would be extruded.
  const activeFeatureId = ref<FeatureId | null>(null);

  /// The sketch currently being edited (drawing tools + entity editing).
  /// null when NOT in sketch-edit mode. This is deliberately separate from
  /// `activeFeatureId` so that left-clicking a sketch in the tree merely
  /// selects it without entering edit mode (SolidWorks behavior).
  const editingSketchId = ref<FeatureId | null>(null);

  /// The feature currently *selected* in the tree (single left-click) —
  /// drives the properties panel. This is a THIRD orthogonal state, kept
  /// distinct from `activeFeatureId` and `editingSketchId` per the
  /// selection≠editing rule (design-principle #2.1): selecting a feature
  /// only shows its properties; it does not start an edit session.
  const selectedFeatureId = ref<FeatureId | null>(null);

  /// Derived view of the currently selected feature. Always reflects the
  /// latest `features` list (design-principle #1/#4): we cache the ID, not
  /// the object, so a `loadState()` refresh can never hand us a stale node.
  const selectedFeature = computed<FeatureNode | null>(() => {
    if (selectedFeatureId.value === null) return null;
    return features.value.find(f => f.id === selectedFeatureId.value) ?? null;
  });

  const activeTool = ref<Tool>("select");
  const selectedId = ref<EntityId | null>(null);
  const isLoading = ref(false);

  // Plugin state
  const plugins = ref<PluginInfo[]>([]);
  const generators = ref<GeneratorInfo[]>([]);
  const activePluginTool = ref<string | null>(null);

  // Viewport state
  const activePlane = ref<string>("xy");
  const showExtrudePanel = ref(false);
  const showDimensions = ref(true);
  const extrudeConfig = ref<ExtrudeConfig>({
    direction: "one_side",
    depth: 1.0,
    dist2: 1.0,
    draft: 0.0,
  });

  /// True only when a sketch is open for editing (drawing tools active).
  /// Selection alone does NOT enter edit mode.
  function isEditingSketch(): boolean {
    if (editingSketchId.value === null) return false;
    const f = features.value.find(x => x.id === editingSketchId.value);
    return f !== undefined && (f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:"));
  }

  function setEntities(data: SketchEntity[]) {
    entities.value = data;
    if (selectedId.value !== null && !data.some(e => e.id === selectedId.value)) {
      selectedId.value = null;
    }
  }

  function setConstraints(data: Constraint[]) {
    constraints.value = data;
  }

  function setFeatures(data: FeatureNode[]) {
    features.value = data;
    // Clear editing state if the editing sketch was deleted.
    if (editingSketchId.value !== null && !data.some(f => f.id === editingSketchId.value)) {
      editingSketchId.value = null;
    }
    // Clear selected feature if it was deleted (prevents ghost selection
    // after cascade deletes, design-principle #4).
    if (selectedFeatureId.value !== null && !data.some(f => f.id === selectedFeatureId.value)) {
      selectedFeatureId.value = null;
    }
    const stillValid = data.some((f) => f.id === activeFeatureId.value);
    if (!stillValid) {
      const firstSketch = data.find((f) =>
        f.feature_type === "Sketch" || f.feature_type.startsWith("Custom:")
      );
      activeFeatureId.value = firstSketch?.id ?? data[0]?.id ?? null;
    }
  }

  function setActiveFeature(id: FeatureId) {
    activeFeatureId.value = id;
  }

  function setEditingSketch(id: FeatureId | null) {
    editingSketchId.value = id;
    if (id === null) {
      // Leaving edit mode resets the tool and entity selection.
      activeTool.value = "select";
      selectedId.value = null;
    }
  }

  function setTool(tool: Tool) {
    activeTool.value = tool;
    selectedId.value = null;
    if (tool !== "plugin") {
      activePluginTool.value = null;
    }
  }

  function select(id: EntityId | null) {
    selectedId.value = id;
  }

  function setPlugins(data: PluginInfo[]) {
    plugins.value = data;
  }

  function setGenerators(data: GeneratorInfo[]) {
    generators.value = data;
  }

  function setActivePluginTool(toolId: string | null) {
    activePluginTool.value = toolId;
    if (toolId) {
      activeTool.value = "plugin";
    }
  }

  /// Set a per-feature color override. The color is a hex string (e.g. "#ff8800").
  /// Currently UI-only — no backend command persists it yet.
  function setFeatureColor(featureId: number, hex: string) {
    featureColors.value = { ...featureColors.value, [featureId]: hex };
  }

  return {
    entities,
    constraints,
    features,
    featureColors,
    activeFeatureId,
    editingSketchId,
    selectedFeatureId,
    selectedFeature,
    activeTool,
    selectedId,
    isLoading,
    plugins,
    generators,
    activePluginTool,
    activePlane,
    showExtrudePanel,
    showDimensions,
    extrudeConfig,
    setEntities,
    setConstraints,
    setFeatures,
    setActiveFeature,
    setEditingSketch,
    setTool,
    select,
    setPlugins,
    setGenerators,
    setActivePluginTool,
    setFeatureColor,
    isEditingSketch,
  };
});