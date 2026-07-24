# EchoCAD 参数化 & 约束化 Pipeline 重构方案

本文档是对 EchoCAD 从纯三角网格管线迁移到参数化 B-rep 管线的完整技术方案。
文档基于 2026-07-22 的深度技术调研编写，包含现状诊断、目标架构、详细设计和分阶段迁移计划。

---

## 1. 执行摘要

EchoCAD 当前的所有实体特征（拉伸、旋转、扫掠、倒角、抽壳、布尔、阵列）均直接输出 `Mesh` 结构——包含 `positions: Vec<f32>`、`normals: Vec<f32>`、`indices: Vec<u32>` 的纯三角面片数据。这导致：

- **倒角是网格级近似**（代码自述 "mesh-level approximation, not a true B-Rep fillet"）
- **边选择用裸顶点索引对 `(u32, u32)`**，每次重生成后失效
- **布尔运算在共面/相同网格场景下不正确**
- **无法导出 STEP（ISO 10303）**
- **无拓扑恒等性**——面、边的身份在重生成后无法保持

**推荐方案**：采用 OpenCASCADE（通过 Rust FFI）作为 B-rep 内核，以"绞杀者模式"（Strangler Fig）逐步替换现有网格管线。全量迁移估算 **20 周**（约 5 个月），其中 Phase 0-2（12 周）即可交付精确倒角和持久化边选择这两个最高价值特性。

关键设计原则：
- **双轨并行**：迁移期间新旧两种管线共存，通过 `use_brep: AtomicBool` 切换
- **持久化命名**：边和面使用基于特征空间坐标的拓扑命名，而非世界空间法向量
- **自适应三角化**：B-rep 在渲染边界按精度要求三角化，前端 Three.js 无需改动
- **版本化缓存**：使用基于依赖版本的失效机制，而非内容寻址哈希

---

## 2. 当前状态：纯网格管线

### 2.1 核心数据结构

```rust
// echi-geom/src/extrude.rs — 唯一的数据交换格式
pub struct Mesh {
    pub positions: Vec<f32>, // 扁平 xyz 三元组
    pub normals: Vec<f32>,   // 扁平 nx ny nz 三元组
    pub indices: Vec<u32>,   // 三角形索引三元组
}
```

所有特征均输出此类型。不存在任何拓扑、参数曲面或持久化几何引用。

### 2.2 特征重生成管线

```
regenerate(doc) → HashMap<FeatureId, Mesh>
   ├─ Sketch → no-op
   ├─ Extrude → 耳切法三角化 2D 轮廓，拼接端盖 + 侧壁
   ├─ Revolve → 旋转剖面点，缝合环带
   ├─ Sweep → 沿路径逐帧放置剖面，Frenet 坐标系拼接
   ├─ Fillet → 二面角阈值 (<30°) 检测锐边，顶点分裂 + 弧面条带
   ├─ Chamfer → 同上，但插入平直面四边面
   ├─ Shell → 角加权法向偏移顶点，复制 + 反转 + 边界封盖
   ├─ Boolean → 三角面片级 CSG（分割 + 环绕数分类）
   ├─ Pattern → 平移/旋转变换复制
   └─ Mirror → 平面反射复制
```

### 2.3 已知局限性（来源：各模块源码注释）

| 模块 | 问题 | 文件 |
|------|------|------|
| Fillet | "mesh-level approximation, not a true B-Rep fillet"，仅对凸边效果好 | `echi-geom/src/fillet.rs` |
| Fillet | 非流形/边界边被静默跳过，面内部不做处理 | 同上 |
| Boolean | 共面重叠面片不产生切割段（平行三角形） | `echi-geom/src/boolean.rs` |
| Boolean | 相同网格的减/交运算结果不正确 | 同上 |
| Boolean | 切割端点严格位于源三角形内部的不会被切割 | 同上 |
| Shell | 凹面区域/厚度超局部特征尺寸时内偏置自相交 | `echi-geom/src/shell.rs` |
| Revolve | 无 world 坐标变换（仅默认 XY 平面正确） | `echi-geom/src/revolve.rs` |
| Sweep | 路径/剖面仅支持直线段，不支持弧线/样条 | `echi-geom/src/sweep.rs` |
| Mass Props | 非水密网格的体积/质心无意义 | `echi-geom/src/mass_props.rs` |
| Extrude | 固定细分密度：圆 64 段、椭圆 48 段、弧 2~256 段 | `echi-geom/src/extrude.rs` |

### 2.4 代码量统计

| 文件 | 行数 | 说明 |
|------|------|------|
| echi-geom/extrude.rs | ~1072 | 网格拉伸（多环支持） |
| echi-geom/fillet.rs | ~564 | 网格级倒角 |
| echi-geom/boolean.rs | ~1093 | 网格 CSG |
| echi-geom/revolve.rs | ~438 | 网格旋转 |
| echi-geom/sweep.rs | ~536 | 网格扫掠 |
| echi-geom/shell.rs | ~300 | 网格抽壳 |
| echi-geom/pattern.rs | ~355 | 网格阵列/镜像 |
| echi-geom/solver.rs | ~1578 | 2D 约束求解器 |
| echi-geom/mass_props.rs | ~150 | 网格质量属性 |
| echi-render/regenerate.rs | ~416 | 特征树求值 |
| echi-core/feature.rs | ~542 | FeatureKind 枚举 |
| **合计** | **~7044** | 待替换或适配 |

---

## 3. 目标架构

### 3.1 Crate 依赖图（迁移后）

```
              ┌──────────────┐
              │  echi-core   │  Document, Feature, Sketch, Parameter, PlaneDefinition
              └──────────────┘
                      ▲
            ┌─────────┼──────────┬──────────────┐
            │         │          │              │
   ┌────────┴───┐  ┌──┴────┐  ┌──┴──────────┐  ┌──┴───────────┐
   │ echi-geom  │  │ echi- │  │ echi-plugin │  │  echi-brep   │
   │            │  │ render│  │             │  │              │
   │ 保留:      │  │       │  │ Plugin trait │  │ Solid, Face, │
   │ - solver   │  │ regen │  │ + registry  │  │ Edge, Vertex  │
   │ - mass     │  │       │  └─────────────┘  │ Boolean,     │
   │ - profile  │  └───▲───┘                   │ Fillet, Shell │
   │ (未来废弃  │      │                       │ Tessellation  │
   │  网格管线) │      │                       └───────┬───────┘
   └─────┬──────┘      │                               │
         ▲             │                               │
         │             │                               │
   ┌─────┴─────────────┴───────────────────────────────┴──┐
   │                 echicad-app                          │
   │  Tauri commands, AppState, UndoManager,              │
   │  BrepRegenerator, MeshRegenerator (legacy)           │
   └──────────────────────────────────────────────────────┘
```

### 3.2 新增 Crate: `echi-brep`

```rust
// ---- 拓扑实体 ----

/// B-rep 实体。拥有一组面、边、顶点的拓扑图及其几何数据。
pub struct Solid {
    pub(crate) faces: Vec<Face>,
    pub(crate) edges: Vec<Edge>,
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) shells: Vec<Shell>,
}

/// 拓扑面：曲面的一个有界区域。
pub struct Face {
    pub id: FaceId,
    pub surface: Surface,            // 平面、圆柱面、锥面、球面、环面、样条面
    pub outer_loop: Vec<CoEdge>,     // CCW 外边界
    pub inner_loops: Vec<Vec<CoEdge>>, // CW 孔洞边界
    pub orientation: bool,           // true = 正向（法向朝外）
}

/// 共边（co-edge）：边在某个面环中的定向使用。
pub struct CoEdge {
    pub edge_id: EdgeId,
    pub reversed: bool,  // 是否与边的 intrinsic 方向相反
}

/// 拓扑边：曲线的有界段。
pub struct Edge {
    pub id: EdgeId,
    pub curve: Curve,                // 直线、圆、椭圆、样条、交线
    pub start_vertex: VertexId,
    pub end_vertex: VertexId,
    pub tolerance: f64,
}

/// 拓扑顶点：3D 空间中的点。
pub struct Vertex {
    pub id: VertexId,
    pub point: [f64; 3],
    pub tolerance: f64,
}

// ---- 稳定拓扑标识 ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaceId(pub u64);
pub struct EdgeId(pub u64);
pub struct VertexId(pub u64);
pub struct ShellId(pub u64);

// ---- 几何曲面 ----

pub enum Surface {
    Plane { origin: [f64; 3], u_dir: [f64; 3], v_dir: [f64; 3] },
    Cylinder { origin: [f64; 3], axis: [f64; 3], radius: f64 },
    Cone { apex: [f64; 3], axis: [f64; 3], half_angle: f64 },
    Sphere { center: [f64; 3], radius: f64 },
    Torus { center: [f64; 3], axis: [f64; 3], major_radius: f64, minor_radius: f64 },
    Extrusion { profile: Vec<[f64; 2]>, direction: [f64; 3] },
    Revolution { profile: Vec<[f64; 2]>, axis_start: [f64; 3], axis_end: [f64; 3] },
    BSplineSurface(BSplineSurfaceData),
}

// ---- 几何曲线 ----

pub enum Curve {
    Line { start: [f64; 3], end: [f64; 3] },
    Circle { center: [f64; 3], axis: [f64; 3], radius: f64 },
    Ellipse { center: [f64; 3], axis: [f64; 3], major_dir: [f64; 3], major_r: f64, minor_r: f64 },
    IntersectionCurve { surface_a: Box<Surface>, surface_b: Box<Surface> },
    BSplineCurve(BSplineCurveData),
}

// ---- 三角化输出 ----

pub struct TessellatedSolid {
    pub face_meshes: Vec<FaceMesh>,
}

pub struct FaceMesh {
    pub face_id: FaceId,
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,    // 来源于精确曲面求导，非三角形叉积
    pub indices: Vec<u32>,
}

pub struct TessellationParams {
    pub linear_deflection: f64,   // 最大弦偏差 (mm)
    pub angular_deflection: f64,  // 最大角度偏差 (rad)
    pub min_edge_length: f64,
    pub max_edge_length: f64,
}
```

### 3.3 `BrepKernel` Trait——内核抽象层

```rust
pub trait BrepKernel: Send + Sync {
    // ---- 创建 ----
    fn extrude(&self, profile: &[Curve2D], height: f64, direction: [f64; 3])
        -> Result<Solid, BrepError>;
    fn revolve(&self, profile: &[Curve2D],
               axis_start: [f64; 3], axis_end: [f64; 3],
               angle_rad: f64) -> Result<Solid, BrepError>;
    fn sweep(&self, profile: &[Curve2D], path: &[Curve3D])
        -> Result<Solid, BrepError>;

    // ---- 修改 ----
    fn fillet_edges(&self, solid: &Solid, edges: &[EdgeId], radius: f64)
        -> Result<Solid, BrepError>;
    fn chamfer_edges(&self, solid: &Solid, edges: &[EdgeId], distance: f64)
        -> Result<Solid, BrepError>;
    fn shell(&self, solid: &Solid, thickness: f64, faces_to_remove: &[FaceId])
        -> Result<Solid, BrepError>;

    // ---- 布尔 ----
    fn union(&self, a: &Solid, b: &Solid) -> Result<Solid, BrepError>;
    fn subtract(&self, a: &Solid, b: &Solid) -> Result<Solid, BrepError>;
    fn intersect(&self, a: &Solid, b: &Solid) -> Result<Solid, BrepError>;

    // ---- 变换 ----
    fn translate(&self, solid: &Solid, delta: [f64; 3]) -> Result<Solid, BrepError>;
    fn rotate(&self, solid: &Solid,
              axis_start: [f64; 3], axis_end: [f64; 3],
              angle_rad: f64) -> Result<Solid, BrepError>;
    fn mirror(&self, solid: &Solid, plane_origin: [f64; 3], plane_normal: [f64; 3])
        -> Result<Solid, BrepError>;

    // ---- 查询 ----
    fn mass_properties(&self, solid: &Solid) -> Result<MassProperties, BrepError>;
    fn tessellate(&self, solid: &Solid, params: &TessellationParams)
        -> TessellatedSolid;

    // ---- IO ----
    fn to_step(&self, solid: &Solid) -> Result<Vec<u8>, BrepError>;
    fn from_step(&self, data: &[u8]) -> Result<Solid, BrepError>;
}
```

Trait 设计遵循**内核无关**原则：应用程序只依赖 `BrepKernel`，具体实现（OpenCASCADE / Truck / 未来纯 Rust 内核）是可插拔的。

---

## 4. 内核选型论证

### 4.1 五个候选方案

| 方案 | 实现量 | 精度 | 功能覆盖 | 未来扩展 | 适合度 |
|------|--------|------|----------|----------|--------|
| **A. 半边 B-rep (OpenCASCADE FFI)** | 中 (集成) | ⭐⭐⭐⭐⭐ | 全覆盖 | STEP/NURBS/装配 | ✅ **推荐** |
| B. 函数表示 (F-rep) | 中 | ⭐⭐⭐ | 布尔好、倒角差 | 有限 | ❌ |
| C. CSG 树 + 惰性求值 | 高 | ⭐⭐⭐⭐ | 布尔好 | STEP 困难 | ⚠️ 备选 |
| D. 混合参数原语 + 网格缓存 | 低 | ⭐⭐⭐ | 有限 | 有限 | ❌ 提升不足 |
| E. 约束图 + 过程式求值 | 中 | ⭐⭐⭐ | 中 | 中 | ❌ |

### 4.2 推荐：OpenCASCADE（绞杀者模式）

**选择理由**：

1. **成熟度**：20 年+工业验证，FreeCAD/KiCad 和大量商业软件在使用
2. **功能覆盖**：涵盖了 EchoCAD 当前和近期需要的每一项操作
3. **STEP 支持**：原生支持 AP203/AP214 导出，为互操作性铺平道路
4. **质量属性**：基于精确曲面积分，非网格近似
5. **维护成本**：内核由 OpenCASCADE 社区维护，我方只需维护 FFI 薄层

**回退方案**：

| 场景 | 方案 |
|------|------|
| OCCT 编译失败（macOS ARM64） | 使用 Homebrew 预编译动态库 + 动态链接 |
| OCCT 不可用 | 回退到 Truck（纯 Rust，MIT 许可证） |
| Truck 也不可用 | 保留当前网格管线 + 增加持久化命名层的降级路径 |

### 4.3 其他内核对比

| 内核 | 优点 | 缺点 | 结论 |
|------|------|------|------|
| **Truck** | 纯 Rust，MIT 许可证 | 缺少倒角/抽壳/扫掠/STEP | 12 个月后重新评估 |
| **Parasolid/ACIS** | 行业标准 | 商业许可证，费用高 | 排除 |
| **自建 B-rep** | 完全掌控 | 5-10 人年开发量 | 不可行 |

---

## 5. 各特征重设计

### 5.1 复杂度矩阵

| 特征 | 当前输出 | 目标输出 | 前端变更 | 迁移难度 | 优先级 |
|------|----------|----------|----------|----------|--------|
| Extrude | Mesh | Solid | 无 | **低** | P0 |
| Revolve | Mesh | Solid | 无 | **低** | P0 |
| Sweep | Mesh | Solid | 无 | **中** | P1 |
| Fillet | Mesh + `(u32,u32)` | Solid + `EdgeId` | 边选择器 | **高** | P0 |
| Chamfer | Mesh + `(u32,u32)` | Solid + `EdgeId` | 边选择器 | **高** | P0 |
| Boolean | Mesh CSG | Solid CSG | 无 | **中** | P1 |
| Shell | Mesh offset | Solid offset | 面选择器(远期) | **中** | P1 |
| Pattern/Mirror | Mesh 副本 | Solid 变换+合并 | 无 | **低** | P2 |
| Mass Props | 网格积分 | 精确积分 | 无 | **极低** | P2 |
| STEP 导出 | 不支持 | Solid → STEP | 文件对话框 | **中** | P2 |
| STEP 导入 | 不支持 | STEP → Solid | 文件对话框 | **高** | P3 |

### 5.2 拉伸（Extrude）— 最高频操作

**当前**：耳切法三角化 2D 环 → 拉伸端盖 + 侧壁三角带 → Mesh

**目标**：
1. 将草图环转换为 `Vec<Curve2D>`（直线段/圆弧/圆/椭圆）
2. 调用 `kernel.extrude(profile, height, direction)`
3. B-rep 内核产出 `Solid`，包含：
   - 1 个平面底面（草图面）
   - 1 个平面顶面（偏移了 height）
   - N 个柱面/平面侧面（每个轮廓边一个）
4. 边 ID 稳定：边 0 始终是第一条轮廓边的顶边，以此类推

**迁移复杂度**：低。`extract_loops()` 已有轮廓提取逻辑，只需添加 `to_curve2d()` 转换。

### 5.3 倒角（Fillet/Chamfer）— 最高影响力变更

**当前**：网格级——通过二面角阈值检测锐边，顶点分裂后插入弧面条带或平面四边面。
边选择存储为 `Vec<(u32, u32)>` 裸顶点索引对。

**目标**：
- `FeatureKind::Fillet { edges: Vec<EdgeId> }` — 使用持久化 B-rep 边 ID
- 调用 `kernel.fillet_edges(solid, &edges, radius)`
- 内核计算相邻面之间的滚球法混合曲面
- 产出新的 `Solid`，原锐边替换为光滑混合面

**边选择 UI 变更**：
1. 后端新增 `get_solid_edges(feature_id)` → 返回边折线 + EdgeId
2. 前端在 3D 视口中渲染可选边（`THREE.LineSegments`）
3. 鼠标悬停时高亮最近边（亮黄色粗线），点击切换选择状态
4. 选中的边以青色线持久高亮
5. 选中列表显示在 `PropertiesPanel` 中

**拓扑命名方案**（关键设计）：

```
不使用世界空间法向量 → 使用特征空间坐标命名

Extrude1.side_face(angle=0.0).edge_with(Extrude1.top_face)
Extrude1.top_face.edge(0)  // 第一条边
```

这样命名的边在上游参数变化（拉伸高度、平移、旋转）时保持有效，
仅在拓扑变化（添加孔洞、修改轮廓）时才失效——这是正确的行为。
参考 FreeCAD 的拓扑命名和 SolidWorks 的持久化 ID 机制。

---

## 6. 约束系统增强

### 6.1 2D 求解器（保留并增强）

当前求解器（Gauss-Newton + Levenberg-Marquardt）保持不变。B-rep 边几何使用精确曲面求值导出，不直接在 2D 求解器中求解。

### 6.2 3D 装配约束（新增）

```rust
pub enum AssemblyConstraint {
    Mate { face_a: FaceId, face_b: FaceId, offset: f64 },
    Flush { face_a: FaceId, face_b: FaceId, offset: f64 },
    Align { axis_a: GeomRef, axis_b: GeomRef },
    Tangent { face_a: FaceId, face_b: FaceId },
    Concentric { edge_a: EdgeId, edge_b: EdgeId },
    Distance { entity_a: GeomRef, entity_b: GeomRef, distance: f64 },
    Angle { entity_a: GeomRef, entity_b: GeomRef, angle_deg: f64 },
}

pub enum GeomRef {
    Face(FaceId),
    Edge(EdgeId),
    Vertex(VertexId),
}
```

装配求解器通过 DOF 分析确定需要移动的实体，按依赖顺序依次求解。

---

## 7. 三角化管线

```
Solid
  └─► 对每个 Face:
        ├─ 在 (u,v) 参数域上自适应采样曲面
        ├─ 将边界 CoEdge 离散为折线
        ├─ 约束 Delaunay 三角化面内部
        │   （边界折线作为约束，内环作为孔洞）
        ├─ 从曲面导数计算法向量（非三角形叉积）
        └─ 产出 FaceMesh { positions, normals, indices }
```

**自适应细化判据**：
1. **弦偏差**：三角形边中点到曲面的距离 > `linear_deflection`
2. **角度偏差**：相邻面法向夹角 > `angular_deflection`
3. **边长**：> `max_edge_length` 时分裂，< `min_edge_length` 时合并

**缓存策略**：
- 使用**基于版本的失效机制**（非内容寻址哈希）
- 每个特征的三角化标记其所有传递依赖的版本向量
- 当特征 N 变化时，版本递增
- 任何引用旧版本的下游特征缓存被失效
- 复杂度 O(深度)，非 O(树大小)

---

## 8. 渲染集成

### 8.1 后端命令（新增/变更）

```rust
// 保持不变（向后兼容）
#[tauri::command]
fn get_solid_mesh(state, feature_id) -> Option<RenderMesh>;

#[tauri::command]
fn get_all_solid_meshes(state) -> Vec<SolidMeshEntry>;

// 新增
#[tauri::command]
fn get_solid_edges(state, feature_id) -> Vec<EdgeInfo>;

#[tauri::command]
fn pick_edge(state, feature_id, screen_x: f64, screen_y: f64) -> Option<EdgeId>;

#[tauri::command]
fn get_face_meshes(state, feature_id) -> Vec<FaceMeshData>;
```

### 8.2 前端 DTO 变更

```typescript
// 新增：单面网格数据（用于面高亮和材质分配）
interface FaceMeshData {
  face_id: number;
  positions: Float32Array;
  normals: Float32Array;
  indices: Uint32Array;
}

// 新增：边信息（用于边选择 UI）
interface EdgeInfo {
  edge_id: number;
  face_a_normal: [number, number, number];
  face_b_normal: [number, number, number];
  polyline: Float32Array;  // 屏幕空间折线顶点
}
```

### 8.3 Three.js 集成

- 每个 `FaceMesh` 构建一个 `THREE.Mesh`（支持面级别的材质/高亮）
- 边渲染使用 `THREE.LineSegments` + `LineBasicMaterial`
- 边拾取使用 CPU 侧射线-线段距离测试（沿用现有 `rayToSegmentDistance` 方案）
- 前端 `UnifiedViewport.vue` 的 `buildGeometry()` 无需改动（仍接收三角形数据）

---

## 9. 分阶段迁移计划

### 阶段总览

| 阶段 | 周数 | 难度 | 风险 | 关键交付 |
|------|------|------|------|----------|
| P0.5: 布尔修复 | 2 | 中 | 低 | 集成 manifold-rs，布尔正确 |
| P0: 基础设施 | 2 | 中 | 中 (OCCT 编译) | echi-brep 编译通过，立方体拉伸验证 |
| P1: 双轨并行 | 2 | 中 | 中 (双路径 bug) | Extrude/Revolve 在 B-rep 模式工作 |
| P2: 倒角迁移 | 4 | **高** | **高** (边选择 UI) | 边选 + B-rep 精确倒角 |
| P3: 布尔/抽壳 | 3 | 中 | 中 (退化输入) | 替换网格 CSG，抽壳不自交 |
| P4: 扫掠/阵列/IO | 3 | 中 | 中 (STEP 兼容) | 全部特征 B-rep，STEP 导出 |
| P5: 增量重生成 | 3 | **高** | **高** (缓存正确性) | 大型零件参数编辑提速 |
| P6: 清理硬化 | 3 | 中 | 低 (回归) | 纯 B-rep，删除网格管线 |
| **总计** | **22 周** | | | |

### P0.5: 布尔修复（先导阶段，2 周）

**目标**：在开始架构重构前修复当前最严重的正确性问题。

**任务**：
1. 集成 `manifold-sys`（MIT 许可证的网格布尔引擎）
2. 用 `manifold` 替换 `boolean.rs` 中的手工 CSG
3. 验证：A - A = 空、A ∩ A = A、球 - 圆柱
4. 添加布尔回归测试套件（≥15 个测试用例）

**验收标准**：所有新布尔测试通过，现有功能不受影响。

### P0: 基础设施（2 周）

**目标**：搭建 `echi-brep` crate 和 OpenCASCADE FFI，不改变任何现有行为。

**任务**：
1. 创建 `crates/echi-brep/`：`Solid`、`Face`、`Edge`、`Vertex`、`BrepKernel` trait
2. 创建 `crates/echi-brep-occt/`：通过 `cxx` 封装 OpenCASCADE
3. 实现 `BrepKernel` 的基础操作（extrude、revolve、tessellate）
4. 添加 Mock `BrepKernel` 实现用于测试
5. Cargo feature flag：`default = ["mesh"]`，`brep = ["echi-brep", "echi-brep-occt"]`
6. `Solid` 序列化（通过 STEP 二进制格式 + zstd 压缩）

**验收标准**：`echi-brep` 编译通过，能创建并三角化一个简单的拉伸立方体。OCCT 在 macOS ARM64 上编译成功。

**回退触发条件**：如果 OCCT 到第 1 周末仍未编译通过，激活 Truck 回退方案。

### P1: 双轨并行（2 周）

**目标**：`RegenResult` 同时存储 `Solid` 和 `Mesh`，Extrude/Revolve 在 `use_brep = true` 时产出 Solid。

**任务**：
1. `RegenResult` 增加 `solids: HashMap<FeatureId, Solid>`
2. 创建 `MeshBrepAdapter`：将现有网格操作封装为 `BrepKernel` trait 实现
3. `AppState` 增加 `use_brep: AtomicBool` 配置开关
4. `regenerate_feature()` 按 `use_brep` 分发到 B-rep 或网格路径
5. `tessellate_for_render()` 将 `Solid` 转换为现有 `RenderMesh` DTO
6. `Command` 层：`getAllSolidMeshes` 对 B-rep 特征执行按需三角化

**验收标准**：Extrude 和 Revolve 在两种模式下产生视觉一致的结果。所有现有测试继续通过。

### P2: 倒角迁移（4 周）

**目标**：用 B-rep 面混合替换网格倒角，实现持久化边选择 UI。

**任务**：

后端（2 周）：
1. `FeatureKind::Fillet { edges }` 从 `Vec<(u32,u32)>` 改为 `Vec<EdgeId>`
2. `FeatureKind::Chamfer { edges }` 同样改为 `Vec<EdgeId>`
3. 旧格式兼容反序列化：旧顶点索引对映射为最近 B-rep 边（附警告）
4. 实现 `kernel.fillet_edges()` 和 `kernel.chamfer_edges()`
5. 新增 `get_solid_edges(feature_id)` 命令
6. 新增 `pick_edge(feature_id, screen_x, screen_y)` 命令

前端（2 周）：
1. 在 `UnifiedViewport.vue` 中添加边选择模式（倒角/倒角创建时进入）
2. 从后端获取边折线，渲染为 `THREE.LineSegments`
3. 实现悬停高亮（亮黄色，0.9 不透明度粗线）
4. 点击切换选择状态（青色持久高亮）
5. 支持框选多条边
6. 选中的边列表显示在 `PropertiesPanel.vue`

**验收标准**：
- 立方体边精确倒角（与 SolidWorks 参考输出对比）
- 圆柱体边倒角
- 边 ID 在参数修改后保持不变
- 旧项目文件中的网格边选择可以打开（可能带迁移警告）

### P3: 布尔和抽壳（3 周）

**目标**：替换网格 CSG 和网格偏移为 B-rep 操作。

**任务**：
1. 替换 `boolean_op()` → `kernel.union/subtract/intersect()`
2. 实现 `kernel.shell()`（通过 OCCT）
3. 添加 `faces_to_remove` 支持（远期可配合面选择 UI）
4. 验证复杂布尔场景：球减圆柱、相交环面、嵌套型腔
5. 可选：移除 `echi_geom::boolean` 模块（或隐藏在 `mesh` feature flag 后）

**验收标准**：
- A - A 产生空 Solid（异常处理）
- A ∩ A 产生与 A 相同的 Solid
- 球 - 圆柱结果与 FreeCAD 参考输出一致
- 抽壳不自相交，壁厚均匀

### P4: 扫掠、阵列、IO（3 周）

**目标**：将剩余特征迁移到 B-rep，添加 STEP 导出。

**任务**：
1. 扫掠迁移：转换剖面/路径为曲线列表，调用 `kernel.sweep()`
2. 阵列/镜像迁移：B-rep 变换 + union
3. 实现 `kernel.to_step()` → STEP AP214 导出
4. 文件菜单添加"导出 STEP"选项
5. 实现 `kernel.mass_properties()` → 精确质量属性
6. 替换 `compute_mass_properties` 命令的实现

**验收标准**：
- 所有特征类型使用 B-rep（可通过 `use_brep` 切换回网格）
- STEP 文件可以被 FreeCAD 正确打开
- 质量属性与 FreeCAD 计算值偏差 < 0.1%

### P5: 增量重生成（3 周）

**目标**：参数修改时仅重计算变更特征的下游，而非整个特征树。

**任务**：
1. 从 `Feature::dependencies()` 构建特征依赖 DAG
2. 参数变更时标记所属特征及其所有传递被依赖者为 "脏"
3. 仅重生成脏特征；干净特征的缓存 `Solid` 复用
4. 基于版本的失效机制：每个特征维护其传递依赖的版本向量
5. 基准测试：50 特征零件参数编辑性能

**验收标准**：
- 修改草图尺寸仅重生成该草图的下游特征
- 大型零件（50+ 特征）参数编辑 < 500ms
- 无陈旧数据泄漏（脏标记传播 100% 正确）

### P6: 清理和硬化（3 周）

**目标**：删除网格代码路径，加固 B-rep 管线，更新文档。

**任务**：
1. 移除 `mesh` feature flag 和 echi-geom 中的网格生成代码
2. 保留 `Mesh` 类型仅用于草图可视化（2D 实体在 3D 视口中的渲染）
3. 移除 `use_brep` 开关——B-rep 成为唯一路径
4. 全量回归测试
5. 性能分析和优化
6. 更新 `architecture.md`、`design-principles.md` 和本文档

**验收标准**：所有 89+ 测试通过，文档更新，无遗留网格代码路径。

---

## 10. 风险登记册

| # | 风险 | 严重度 | 可能性 | 缓解措施 |
|---|------|--------|--------|----------|
| R1 | **OpenCASCADE 编译集成失败** | 🔴 关键 | 中 | Homebrew 预编译动态库；Truck 回退方案；3 天技术验证窗口 |
| R2 | **EdgeId 不稳定** | 🔴 关键 | 中 | 基于特征空间的拓扑命名；FreeCAD TNaming 机制；最佳努力边重映射；集成测试覆盖 |
| R3 | **性能退化** | 🔴 关键 | 中 | 增量重生成 (P5)；三角化缓存；LOD 自适应三角化；每阶段 50 特征基准测试 |
| R4 | **双表示代码膨胀** | 🟠 高 | 高 | P0-P4 期间的临时成本；独立的迁移工具离线转换文件格式；阶段间 89+ 测试回归做门禁 |
| R5 | **边选择 UX 不佳** | 🟠 高 | 中 | 悬停高亮 + 粗线；框选支持；选择循环机制；2-3 名工程师可用性测试 |
| R6 | **LaftSolid 工作量低估** | 🟠 高 | 中 | 从 P4 移除 Loft，仅支持 RuledLoft（平行平面+等顶点数）；完全的 Loft 推迟到 P6+ |
| R7 | **OCCT LGPL 许可证合规** | 🟡 中 | 低 | 动态链接（.dylib）；License 文件随应用分发；About 对话框标注 |
| R8 | **Project 文件兼容性** | 🟡 中 | 低 | 格式版本 v1→v2；旧格式加载+迁移；`Save As Legacy` 选项；迁移测试 |
| R9 | **退化几何导致崩溃** | 🟡 中 | 中 | 所有内核调用包装在 `catch_unwind`；操作前输入验证（radius>0, distance>epsilon）；Solid 有效性校验 |
| R10 | **开发者巴士因子** | 🟢 低 | 高 | 每个公开类型和方法写 doc comment；ADR 记录架构决策；`BrepKernel` trait 保持小且文档完善 |

---

## 11. 需要进一步验证的开放问题

### Q1: OpenCASCADE 在 macOS ARM64 上的编译可行性
- OCCT 是否能不打补丁编译？
- 静态 vs 动态链接？
- 最小模块集是什么？（约需 ModelingData、ModelingAlgorithms、DataExchange 三个模块）
- **验证方式**：3 天内创建一个 `echi-brep-occt` 最小示例

### Q2: Truck 是否可作为替代内核？
- 截至 2026 年中，Truck 是否已支持倒角/抽壳/扫掠？
- Truck 的布尔运算健壮性如何？
- **验证方式**：2 天内编写 Truck vs OCCT 对比测试（5 个代表性操作）

### Q3: OCCT 三角化 vs 现有网格生成的性能
- 典型拉伸零件（如齿轮插件输出）的三角化时间？
- 交互帧率所需的三角化质量参数？
- **验证方式**：P0 中进行基准测试

### Q4: 旧项目文件中的网格边选择迁移
- 最佳努力映射 `(vertex_index_a, vertex_index_b)` → `EdgeId` 的准确率？
- 是否需要只读的旧格式模式？
- **验证方式**：P1 中实现映射器，在所有现有项目文件上测试

### Q5: OCCT LGPL 许可证
- 动态链接是否满足 Tauri 桌面应用的 LGPL 要求？
- 是否需要提供 OCCT 库替换机制？
- **验证方式**：1 天内咨询开源许可证资源，参考 FreeCAD 实践

### Q6: 是否需要自建 Rust B-rep 内核？
- **初步答案**：否。最小内核（拉伸+旋转+布尔）需要 12-18 人月；完整内核 5-10 人年。独立开发者不可行。
- 如有社区贡献，未来可考虑自建内核以消除 C++ 依赖。

---

## 12. 相关文档

- [架构总览](./architecture.md) — 当前 crate 划分与数据流
- [设计原则](./design-principles.md) — 重构过程中应遵循的开发规范
- [交互指南](./interaction-guide.md) — 用户视角的功能说明

---

*最后更新：2026-07-22 | 版本：v1.0-draft*
