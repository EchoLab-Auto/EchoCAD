# EchoCAD 设计原则

本文档沉淀 EchoCAD 在 0.2 / 0.3 迭代中踩过的坑与由此确立的设计原则。
所有新功能开发都应遵循这些约定，避免重蹈覆辙。

---

## 1. 单一数据源（Single Source of Truth）

**原则：文档（`Document`）是唯一的真实状态，存于 Rust 后端。前端只是它的投影。**

- 任何"实体长什么样"的问题，答案必须能从 `Document` 推导出来。
- 前端 store（Pinia）只存 *展示状态*：当前工具、选择、面板开关、活动平面。
- 前端不缓存任何后端能算出的东西（实体坐标、特征树、参数值），每次需要时重新拉取。

**反例（0.2 的 bug）：** 前端把 `FeatureNode` 缓存在 `selectedFeature: ref` 里，
`loadState()` 之后这个引用还指向旧对象，导致属性面板显示过期数据。

**正确做法：** 缓存 *ID*，用 computed 派生对象。

```typescript
// ❌ 错误
const selectedFeature = ref<FeatureNode | null>(null);

// ✅ 正确
const selectedFeatureId = ref<FeatureId | null>(null);
const selectedFeature = computed(() =>
  sketchStore.features.find(f => f.id === selectedFeatureId.value) ?? null
);
```

---

## 2. 同步不变量（Sync Invariants）

**原则：每次文档变更必须同时刷新三个面——特征树、3D 视口、草图实体层。少一个就是 bug。**

后端的每次 mutation 都会触发 `regen_locked` 重建 `regen_result`。前端拿到最新数据需要 *两次* 拉取：

```typescript
// ✅ 标准 mutation 流程
async function mutateSomething() {
  await someTauriCommand();           // 1. 后端改文档 + 重建 regen_result
  await loadState();                  // 2. 拉 features + sketch entities
  await unifiedViewport.value?.refreshViewport();  // 3. 拉 solids
}
```

**0.2 的具体 bug：** `deleteFeature` 只调了 `loadState()`，没调 `refreshViewport()`，
导致被删除的特征对应的 mesh 在视口里残留。

**记忆口诀：** 改文档 → `loadState()` + `refreshViewport()`，缺一不可。

---

## 2.1 选中 ≠ 编辑（Selection ≠ Editing）

**原则：一个特征可以处于三种正交状态，必须用独立的状态位表示，不能复用同一个字段。**

| 状态 | 字段 | 触发 | 效果 |
|------|------|------|------|
| 选中 | `selectedFeatureId` | 左键单击树节点 | 属性面板显示该特征 |
| 活动 | `activeFeatureId` | 选中即设 | 特征操作（拉伸/旋转）的目标 |
| 编辑中 | `editingSketchId` | 双击 / 右键「编辑草图」 | 显示草图实体 + 启用绘制工具 |

**反例（0.3 之前）：** 只有 `activeFeatureId` 一个字段，`isEditingSketch()` 判断
"active 特征是不是草图"。结果左键点选草图就进入编辑模式——用户只是想看属性，
却被强制切到平面视图。

**正确做法：**
- `isEditingSketch()` 只看 `editingSketchId`，与选中无关
- 左键 `selectFeature` 只设 `selectedFeatureId` + `activeFeatureId`
- 双击 / 右键菜单调 `enterSketchEdit(id)` 才设 `editingSketchId`
- 切换选中特征时自动退出编辑（`editingSketchId !== id → exitSketchEdit()`）
- `setFeatures` 检测 `editingSketchId` 失效（被删）时清空

**特征操作的目标草图：** `editingSketchId ?? (activeFeature 是草图 ? activeFeatureId : null)`，
这样选中草图不进入编辑也能直接拉伸/旋转。

---

## 3. 变更 vs 选择（Mutation vs Selection）

**原则：只有文档变更才产生 undo 快照。选择状态、工具切换、活动草图切换都不产生。**

| 操作 | 是否 snapshot | 理由 |
|------|--------------|------|
| 添加 / 删除 / 修改实体 | ✅ | 文档变了 |
| 添加 / 删除 / 抑制特征 | ✅ | 文档变了 |
| 改参数值 | ✅ | 文档变了 |
| 切换活动草图 (`set_active_sketch`) | ❌ | 只是 UI 状态 |
| 切换工具（直线 / 圆 / 选择） | ❌ | 只是 UI 状态 |
| 切换活动平面 | ❌ | 只是 UI 状态 |
| 相机移动 / 缩放 | ❌ | 只是视图状态 |

**反例：** `set_active_sketch` 之前会 `snapshot()`，意味着每次点选特征树中的草图
都会污染 undo 栈。用户按 Ctrl+Z 期望撤销上次的实体绘制，结果只是在两个
草图之间反复横跳。

---

## 4. 派生状态优于命令式同步（Derived > Imperative）

**原则：能从其他状态算出来的，绝不手动赋值维护。**

| 状态 | 类型 | 来源 |
|------|------|------|
| `isSketchMode` | computed | `activeFeatureId` + `features[].feature_type` |
| `isDrawing` | computed | `isSketchMode` + `activeTool` |
| `selectedEntity` | computed | `selectedId` + `entities[]` |
| `selectedFeature` | computed | `selectedFeatureId` + `features[]` |
| `regenErrorCount` | computed | `features[].errors` |

命令式赋值是同步 bug 的温床。任何 `xxx.value = ...` 都应该审视一下：
这能不能改成 computed？

---

## 5. 坐标系统一（Plane Consistency）

**原则：绘制平面、拉伸平面、视口对齐平面必须一致。每个草图特征自带 plane，前端切换草图时必须同步。**

SolidWorks 的做法：草图永远 *属于* 一个平面。切换草图 → 视口切到那个平面。

**0.3 之前的 bug：** 前端用全局 `activePlane` 画草图，后端用每个 `Sketch` 特征
存储的 `plane` 做拉伸。如果用户在 YZ 草图上工作但 `activePlane` 是 `xy`，
绘制发生在 XY，拉伸发生在 YZ，结果就是 3D 错位。

**正确做法：**
- `FeatureNode.plane` 由后端暴露
- `selectFeature()` 同步 `sketchStore.activePlane = feature.plane`
- `newSketch()` 用当前 `activePlane` 创建，再通过 `selectFeature()` 完成同步

---

## 6. 响应式与原生对象的边界（Reactivity Boundary）

**原则：Vue 的 Proxy 不能跨界到 Three.js / WebGL / 原生对象。用 `markRaw` / `shallowRef` 划清界限。**

```typescript
// ❌ 错误：THREE 对象被 Proxy 包装后内部状态损坏
const ctx = ref<ThreeContext | null>(null);
ctx.value = { scene, camera, renderer, controls, ... };

// ✅ 正确：markRaw 阻止 Proxy
import { shallowRef, markRaw } from "vue";
const ctx = shallowRef<ThreeContext | null>(null);
ctx.value = markRaw({ scene, camera, renderer, controls, ... });
```

---

## 6.1 前后端状态的"读写一致性"（Read-Your-Write）

**原则：前端写入后端后，如果后续逻辑要基于这个写入做判断，必须先重新拉取。**

```typescript
// ❌ 错误：写完后立刻读，但读的是旧缓存
const newId = await addPoint(x, y);          // 后端已写入
state.circleCenterId.value = newId;
// 后面 getPointCoords(newId) 从 store.entities 查——但 store.entities 还是上次的！
if (center) await addCircle(state.circleCenterId.value, r);  // center === null，圆被静默丢弃

// ✅ 正确：写完后立刻 refresh
const newId = await addPoint(x, y);
state.circleCenterId.value = newId;
await refreshSketch();                        // 同步 store.entities
// 现在 getPointCoords(newId) 能拿到数据
```

**为什么这容易发生：** Pinia store 是后端的"读缓存"。`invoke()` 直接调用后端写入，
但 store 不会自动感知。任何后续依赖 store 数据的逻辑都必须先 `refreshSketch()`。

**自查方法：** 看到一个 `add*` / `delete*` / `update*` 命令后，
跟踪下一个读操作是不是用了 store 里的旧数据。


**更深一层的坑：** 不要把 THREE 对象作为 props 传给子组件——props 在传递过程中
也会被响应式化。0.3 之前的 `DimensionOverlay` 接收 `camera: THREE.PerspectiveCamera`，
导致 renderer 进程崩溃。

**正确做法：** 传函数，不传对象。

```typescript
// ❌ 错误
<DimensionOverlay :camera="ctx.camera" :renderer="ctx.renderer" />

// ✅ 正确
<DimensionOverlay :project="(sx, sy) => worldToScreen(...)" />
```

---

## 7. 依赖与级联（Dependency & Cascade）

**原则：特征树的依赖关系必须显式建模，删除前必须检查。**

`Feature::dependencies()` 返回上游 feature ID 列表。再生管线据此：
1. 校验依赖存在
2. 依赖失败则跳过下游，记录 error
3. 删除时检测 `dependents_of(id)`，非空则需用户确认级联删除

**反例（0.2 之前）：** 删除草图后，引用它的拉伸特征静默失败——
没有错误提示，没有级联选项，只是视口里什么都没有。

**正确做法：**
- 删除命令返回 `Err("dependents:name1,name2")`
- 前端解析并弹出确认对话框
- 确认后用 `cascade: true` 重试

---

## 8. 错误要可见（Errors Must Surface）

**原则：再生失败不能静默。每个特征的错误要落到 `FeatureNode.errors`，UI 必须显示。**

```
FeatureNode.errors: Option<String>
   ↓
特征树图标 ⚠ + 红色背景
   ↓
属性面板红色错误框
   ↓
侧栏顶部错误计数徽章
```

**反例：** 早期版本 `regenerate` 失败时只是不插入 mesh，用户看到的是
"特征创建成功了但视口里什么都没有"，完全无法定位问题。

---

## 9. 命令的薄与厚（Thin Commands, Thick Domain）

**原则：Tauri command 是薄壳，所有业务逻辑在 crate 里。**

```rust
// ✅ 好：command 只做 snapshot + 锁 + 调用 crate + regen
#[tauri::command]
pub fn add_extrude_feature(...) -> Result<FeatureId, String> {
    add_feature_with_param(&state, "Extrude", d, |_id, param_id| {
        FeatureKind::Extrude { ... }
    })
}
```

```rust
// ❌ 坏：command 里塞满参数构造、错误处理、命名规则、重生成
```

**收益：**
- crate 可独立测试，不需要起 Tauri
- 命令层统一通过 `add_feature_with_param` / `add_feature_kind` 两个 helper 走，
  自动获得 snapshot、参数分配、命名、依赖校验、regen 五个能力

---

## 10. 命名与约定

- `Feature.id` / `Feature.name` 是元数据，`FeatureKind` 是行为——分离后访问器无需 match
- 特征默认名：`{Kind}{id}`（如 `Extrude3`），用户在树中重命名不影响 id
- 参数默认名：与所属特征同名（如 `Extrude3` 的深度参数也叫 `Extrude3`），便于回溯

---

## 11. 测试金字塔

| 层级 | 内容 | 现有 |
|------|------|------|
| 单元测试 | crate 内部（extrude / fillet / solver / document） | ✅ 19 个 |
| 集成测试 | Tauri command 层 | ❌ 待补 |
| E2E | 前端 + 后端联动 | ❌ 手动验证 |

新加几何时优先在 crate 层写单元测试（不依赖 Tauri），至少覆盖：
- 正常路径（能生成 mesh）
- 边界（空输入 / 退化输入）
- 不变量（无 NaN、索引在界内）

---

## 12. 提交前检查清单

写完一个 mutation 后问自己：

- [ ] 后端 command 是否调用了 `regen_locked`？
- [ ] 前端调用后是否 `loadState()` + `refreshViewport()` 都跑了？
- [ ] 错误是否被捕获并 toast 给用户？
- [ ] 是否会污染 undo 栈（非 mutation 也 snapshot 了）？
- [ ] 选择状态是否在变更后还有效（selectedId / activeFeatureId / activePlane）？
- [ ] THREE 对象是否被 Proxy 了（ref vs shallowRef/markRaw）？
- [ ] **新功能是否同时有 Tauri command + Agent API 封装（原则 #14）？**
- [ ] **Agent API 的参数是否与 command 签名一一对应（编译期类型检查）？**

---

## 13. 迁移兼容性（Migration Compatibility）

**原则：迁移期间新旧管线必须共存，通过 feature flag 切换，绝不能出现"中间状态不可用"。**

在参数化 B-rep 管线迁移过程中（详见[参数化重构方案](./parametric-refactor-plan.md)）：

- 所有几何操作通过 `BrepKernel` trait 抽象
- `AppState.use_brep: AtomicBool` 控制分发路径
- 旧网格路径保留为降级回退，直到 P6 阶段才删除
- 文件格式版本号 (`format_version`) 区分 v1（网格）和 v2（B-rep）格式
- 旧项目文件可以通过独立的迁移工具批量升级

**反例（风险）**：在特性迁移到一半时删除旧 FeatureKind 变体，导致中间版本无法打开现有项目文件。

---

## 14. API 完备性（API Parity）

**原则：每一个建模步骤都必须能通过编程 API 完成。UI 只是 API 的一个客户端，不是唯一的客户端。**

EchoCAD 的调用链是三层：

```
┌─────────────────────────────────────────────────────┐
│  客户端层（任意数量）                                  │
│  ├ Vue UI（人）          — UnifiedViewport / Toolbar │
│  ├ Agent API（AI agent） — app/src/commands/agent-api.ts │
│  └ 测试脚本 / 自动化      — 直接调 commands/sketch.ts    │
├─────────────────────────────────────────────────────┤
│  IPC 层 — Tauri commands（app/src-tauri/commands.rs） │
├─────────────────────────────────────────────────────┤
│  领域层 — echi-core / echi-geom / echi-render / …     │
└─────────────────────────────────────────────────────┘
```

**规则：**

1. **UI 不做 API 做不了的事。** 每个交互操作（绘制、约束、特征、阵列、导出、测量……）
   必须对应一个 Tauri command，并且必须在 `agent-api.ts` 中有对应封装。
2. **新功能 = API + UI 一起交付。** 只有 UI 按钮没有 API 的功能视为未完成。
3. **API 优先，UI 随后。** 设计新功能时先确定命令签名（参数、返回值、错误），
   UI 围绕命令构建；不要把 UI 交互序列（"先点这个再点那个"）硬编码为唯一路径。
4. **参数完备。** API 必须暴露 UI 能调的所有参数（例如拉伸的 `selected_regions`、
   圆角/倒角的 `edges` 边列表），不允许"UI 能传但 API 传不了"的参数。
5. **可观察性。** Agent 需要读取状态的 API 与写入 API 同等重要：
   `get_features` / `get_sketch_entities` / `get_regen_errors` /
   `get_mass_properties` / `capture_viewport`（视觉自检）。

**覆盖检查方法（新增命令时自查）：**

```
Tauri command (snake_case)     Agent API (camelCase)        状态
─────────────────────────────  ──────────────────────────   ────
add_extrude_feature            extrude / extrudeTwoSides …   ✅
add_fillet_edges_feature       filletEdges                   ✅
get_extrude_regions            getExtrudeRegions             ✅
export_stl / obj / gltf        exportStl / Obj / Gltf        ✅
generate_plugin_feature        generatePluginFeature         ✅
measure_angle                  measureAngle                  ✅
…（完整对照表见 agent-api.ts 头部注释）
```

**为什么这重要：**
- AI agent 是 EchoCAD 的一等用户。视觉自检循环（建模 → `captureViewport` →
  多模态检查 → 修正）要求每个步骤都可编程触发。
- 自动化测试可以用 Agent API 驱动完整建模流程，不需要启动 UI 交互层。
- UI 重构（如 HomeView 拆分为 composable）不会破坏外部客户端。

**反例（0.3 之前）：** Agent API 的 `line()` 直接传坐标给期望 EntityId 的
`add_line` 命令——因为 UI 从不这样调用，这个错位直到 agent 实际使用才暴露。
教训：API 封装必须与命令签名同步编译检查，且要有冒烟测试覆盖。

---

## 相关文档

- [架构总览](./architecture.md) — crate 划分与数据流
- [交互指南](./interaction-guide.md) — 用户视角的功能说明
- [参数化重构方案](./parametric-refactor-plan.md) — B-rep 管线迁移的完整技术方案
