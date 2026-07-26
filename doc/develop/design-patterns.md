# EchoCAD 设计模式

> 草图和拉伸系统中应用的 9 个核心设计模式，从底层几何到上层交互，按层次排列。

---

## 目录

| # | 模式 | 层次 | 一句话 |
|:-:|------|:--:|--------|
| 1 | [参数化曲线模式](#1-参数化曲线模式) | 几何 | 曲线在分类前不离散，网格生成时才自适应采样 |
| 2 | [桥接边模式](#2-桥接边模式) | 几何 | 退化边界将孔洞连接到外环，耳部裁剪自然绕过 |
| 3 | [管线模式](#3-管线模式) | 架构 | 提取→分类→采样→剖分→网格，每阶段独立可替换 |
| 4 | [外环—孔洞—岛屿分类](#4-外环孔洞岛屿分类) | 算法 | 面积排序 + 最小包含环检测实现三层嵌套 |
| 5 | [特征类型枚举模式](#5-特征类型枚举模式) | 数据 | Rust enum 携带结构化数据 + 穷尽式 match 分发 |
| 6 | [插件策略模式](#6-插件策略模式) | 扩展 | Plugin trait + 注册表 + 特征携带来源元数据 |
| 7 | [工具状态机模式](#7-工具状态机模式) | 交互 | 统一状态结构驱动多工具多步交互 |
| 8 | [吸附优先级模式](#8-吸附优先级模式) | 交互 | 9 级优先级 + 角度吸附例外规则 |
| 9 | [特征树映射模式](#9-特征树映射模式) | 前后端 | 类型字符串编码 + 条件渲染解耦前后端 |

---

## 模式关系图

```
 ┌──────────────────┐
 │ 5. 特征类型枚举   │ ← 数据模型层
 └────────┬─────────┘
          │ 定义 FeatureKind 变体
          ▼
 ┌──────────────────┐     ┌──────────────────┐
 │ 6. 插件策略      │────→│ 9. 特征树映射     │ ← 扩展 + 前后端
 └────────┬─────────┘     └──────────────────┘
          │ 调用
          ▼
 ┌──────────────────┐
 │ 1. 参数化曲线    │ ← 几何层
 ├──────────────────┤
 │ 2. 桥接边        │
 ├──────────────────┤
 │ 4. 外环-孔洞-岛屿│
 └────────┬─────────┘
          │ 组合为
          ▼
 ┌──────────────────┐
 │ 3. 管线           │ ← 架构层
 └──────────────────┘
          │
          ▼
 ┌──────────────────┐
 │ 7. 工具状态机    │ ← 交互层
 ├──────────────────┤
 │ 8. 吸附优先级    │
 └──────────────────┘
```

---

## 1. 参数化曲线模式

### 层次

几何层 — `crates/echi-geom/src/param_curve.rs`

### 要解决的问题

传统做法在轮廓提取阶段就将所有曲线离散为固定分段多边形。离散化后的面积、质心、包含检测全部在多边形逼近值上计算，精度随半径漂移，大半径侧壁可见棱角。

### 核心思想

**推迟离散化**。曲线在提取后保持参数形式，分类阶段全部使用解析公式。仅在网格生成前的最后一刻才按弦误差自适应采样。

### 结构

```
ParamCurve（枚举）
├── Line       { 起点, 终点 }
├── Arc        { 圆心, 半径, 起始角, 终止角 }
├── Circle     { 圆心, 半径 }
└── EllipseArc { 圆心, 长轴, 短轴比, 参数范围 }

ParamLoop = Vec<ParamCurve>   （闭合参数环）
```

### 关键设计

- **存储效率**：圆 → 4 个 f64（圆心+半径），旧方案 → 128 个 f64（64 对坐标）
- **面积计算**：用 Green 定理线积分——圆用解析 πr²，弧用扇形+三角形，线用端点叉积——无浮点累加误差
- **包含检测**：从点向右发水平射线，与参数曲线求精确交点——线与线用参数方程，圆用二次求根，弧加角度过滤
- **自适应分段**：分段数 = 角度跨度 / acos(1 − 弦误差/半径)，夹在 4~512 段。小圆省资源，大圆保精度

### 数据流对比

```
旧：提取 → 离散化(64段) → 分类 → 三角剖分 → 网格
新：提取 → 分类(解析) → 采样(自适应) → 三角剖分 → 网格
```

---

## 2. 桥接边模式

### 层次

几何层 — `crates/echi-geom/src/extrude.rs`

### 问题

耳部裁剪只能处理简单多边形，无法处理带孔洞的截面（如矩形板打圆孔）。

### 核心思想

将每个孔洞通过一条"桥"连接到外环上，把孔洞顶点引入外环序列，形成退化单一边界。耳部裁剪在这个退化多边形上运行时，桥两侧三角形抵消，孔洞自然留空。

### 结构

```
输入：外环顶点 + N 个孔洞顶点列表

步骤 1：统一编号 — [外环 0..M-1, 孔洞1 M..M+N₁-1, …]
步骤 2：初始化多边形索引 = [0..M-1]          ← 只含外环！
步骤 3：对每个孔洞（逆序）：
        a. 找最近的外环顶点 p_bridge
           — 跳过任何孔洞顶点
           — 跳过已被其他孔洞桥接过的顶点
        b. 插入桥：[p_bridge, 孔洞顶点[全], 孔洞首, p_bridge]
        c. 记录 p_bridge 为已使用
步骤 4：耳部裁剪 → 三角形索引
```

### 三个关键约束（修过的 bug）

| 约束 | bug | 后果 |
|------|-----|------|
| 初始多边形只含外环 | 旧代码把孔洞也放入初始序列 | 孔洞区域被重复三角化，底面积多 33% |
| 多孔洞桥接点互斥 | 未跟踪已桥接顶点 | 两座桥共享同一点，拓扑混乱 |
| 孔洞顶点显式反转为顺时针 | Circle 无内在方向，reverse() 参数环无效果 | 孔洞采样多边形为 CCW，被当实体 |

---

## 3. 管线模式

### 层次

架构层 — `crates/echi-geom/src/extrude.rs`

### 问题

拉伸操作需经过提取、分类、采样、剖分、网格生成五个阶段。参数化和多边形两条路径在前两阶段有不同实现，但下游阶段可复用。

### 核心思想

将管线分解为独立阶段，每阶段有明确的输入/输出类型契约。切换路径只需替换对应阶段的实现。

### 五阶段管线

```
Sketch
  │
  ▼ 阶段 1：提取          extract_param_loops()  →  ParamLoop 列表
  │
  ▼ 阶段 2：分类          面积排序 + 最小包含环检测 → 外环 + 孔洞分组
  │
  ▼ 阶段 3：采样          sample_loop(chord_error) → 点列表
  │
  ▼ 阶段 4：剖分          triangulate_with_holes() → 三角形索引
  │
  ▼ 阶段 5：网格          extrude_with_holes()     → Mesh（底面+顶面+侧壁）
  │
  ▼ 变换                  transform_mesh_to_world() → 世界坐标
```

### 两路径的阶段差异

| 阶段 | 多边形路径 | 参数化路径 |
|------|-----------|-----------|
| 提取 | 圆→64 段多边形 | 圆→`Circle {center, radius}` |
| 面积 | 鞋带公式（n 次浮点累加） | Green 定理（解析 πr²） |
| 包含 | 射线×多边形边（边越多越准） | 射线×参数曲线（二次求根，精确） |
| 采样 | 已在提取阶段完成 | 此阶段按弦误差自适应 |

---

## 4. 外环—孔洞—岛屿分类

### 层次

算法层 — `crates/echi-geom/src/extrude.rs`

### 问题

多个闭合环之间存在三种嵌套关系：外环（实体）、孔洞（挖空）、岛屿（孔中独立体）。简单的"质心在外环内→孔洞"无法区分孔洞和岛屿。

### 核心思想

**面积排序 + 最小包含环检测**。不为每个环找"任何包含它的外环"，而是找**面积最小的包含环**。如果最小包含环是孔洞，当前环就是岛屿（新外环）。

### 分类规则

```
对每个环，找面积最小且包含其质心的已登记环：
  ├── 找不到             → 新外环
  ├── 找到，且是外环      → 该外环的孔洞
  └── 找到，且是孔洞      → 新外环（孔中岛屿）
```

### 示例

| 草图 | 分类 |
|------|------|
| 同心圆 R=5, R=2 | 外环(5) + 孔洞(2) |
| 两个分离圆 | 外环₁ + 外环₂ |
| 矩形 + 两个圆孔 | 外环(矩形) + 孔洞₁ + 孔洞₂ |
| 三层同心 R=5,3,1 | 外环(5) + 孔洞(3) + 岛屿(1) |

---

## 5. 特征类型枚举模式

### 层次

数据模型层 — `crates/echi-core/src/feature.rs`

### 问题

系统需要管理 14 种特征类型，每种携带不同字段、不同再生逻辑。继承或 trait 对象会导致动态分发开销和类型丢失。

### 核心思想

利用 Rust `enum` 每个变体内联携带专属数据。编译器穷尽式检查保证所有处理环节覆盖所有类型。

### 变体分类

```
FeatureKind
├── 草图形（内联携带 Sketch）
│   ├── Sketch        { sketch, plane }
│   ├── SketchModule  { sketch, plane, source }
│   └── CustomSketch  { sketch, custom, plane }
├── 基于草图的造型
│   ├── Extrude  { sketch_id, distance, direction, … }
│   ├── Revolve  { sketch_id, angle, axis }
│   └── Sweep    { profile_id, path_id }
├── 编辑特征（作用于已有实体）
│   ├── Fillet   { target_id, radius, edges }
│   ├── Chamfer  { target_id, distance, edges }
│   ├── Shell    { target_id, thickness }
│   └── Boolean  { op, target_a, target_b }
├── 阵列/镜像
│   ├── LinearPattern   { target_id, direction, count, spacing }
│   ├── CircularPattern { target_id, axis, count, angle }
│   └── Mirror          { target_id, plane }
└── 插件直出
    └── CustomSolid { sketch_id, distance, custom }
```

### 处理环节的全覆盖

| 环节 | 位置 | 机制 |
|------|------|------|
| 访问器 | `feature.rs` — `is_sketch()`, `sketch()`, `plane()`, `dependencies()` | 多分支合并 |
| 再生 | `regenerate.rs` — `regenerate_feature()` | 每变体一条路径 |
| 前端映射 | `commands.rs` — `feature_to_node()` | 变体→`feature_type` 字符串 |
| 图标 | `FeatureTree.vue` — `featureIcon()` | 字符串→唯一图标 |
| 右键菜单 | `ContextMenu.vue` — `isSketchLike` | 字符串→条件显示 |

编译器强制检查：漏掉任何一个 match 分支都会编译报错。

---

## 6. 插件策略模式

### 层次

扩展层 — `crates/echi-plugin/src/types.rs`

### 问题

齿轮、摆线等外部模块各自以不同算法生成草图几何，需要统一接入方式。

### 核心思想

`Plugin` trait 定义生成接口，`PluginRegistry` 统一管理。特征创建时通过 `CustomFeatureData` 记录来源信息，前端据此展示参数和图标。

### 调用链

```
Plugin trait
  ├── generators()        → 声明能力 + 参数定义
  ├── generate_sketch()   → 返回 Sketch（参数化几何）
  └── generate_solid()    → 返回 Mesh（可选）

注册: PluginRegistry.register(GearPlugin)
发现: list_generators → 前端插件列表
调用: generate_plugin_feature(plugin_id, generator_id, params)
  → PluginRegistry.generate_sketch()
  → 创建 FeatureKind::SketchModule { sketch, source }
  → 特征树: ⚙️ Spur Gear_N
```

### 来源元数据

`CustomFeatureData { plugin_id, generator_id, params }` 存储在 `SketchModule.source` 中：
- `plugin_id` + `generator_id` → 生成 `feature_type = "Module:echi.gear:spur_gear"`
- `params` → 属性面板展示只读参数
- 关键词匹配 → 特殊图标

---

## 7. 工具状态机模式

### 层次

交互层 — `app/src/composables/useSketchInteraction.ts`

### 问题

9 种草图工具各有不同交互步骤（直线 2 步链式、圆 2 步、弧 3 步、裁剪/延伸 1 步），中间状态各不相同但共享鼠标事件入口。

### 核心思想

所有工具的状态集中在单一结构 `SketchInteractionState` 中。`handleDrawingMouseDown` 是统一事件入口，通过 `switch(activeTool)` 分发。状态迁移由字段的 `null → value → null` 驱动。

### 状态字段组织

```
SketchInteractionState
├── 直线: lineStartId, lineStartPos, lineChainOriginId
├── 圆:   circleCenterId, circleCenterPos
├── 弧:   arcCenterId, arcRadiusSet, arcRadiusPoint
├── 矩形: rectStart
├── 样条: splinePoints
├── 椭圆: ellipseCenterId, ellipseCenterPos
└── 通用: pendingPointIds, activeSnap, isDragging
```

### 状态迁移模式

所有工具遵循同一模式：
```
click₁ → 设置字段（已有实体 ID 或延后坐标）
click₂₊ → 验证状态 → 创建实体 → 更新字段（链式）或重置（单次）
Esc/右键 → 清理孤儿点 → resetDrawingState → 清预览
```

### 延后创建

首次点击不立即创建 Point 实体——坐标暂存，真正创建在第二次点击。取消时零副作用（无需 undo）。

---

## 8. 吸附优先级模式

### 层次

交互层 — `app/src/composables/useSketchInteraction.ts` + `UnifiedViewport.vue`

### 问题

光标同一位置可能触发多种吸附，需选最合理的一个，且避免角度吸附干扰闭合操作。

### 核心思想

收集所有候选项 → 按优先级排序 → 同级按距离 → 取最优。吸附到已知几何时禁用角度吸附。

### 优先级定义

```
优先级  类型            说明
  0    endpoint        已有几何点 — 最精确
  1    center          圆心
  2    midpoint        线段中点
  3    intersection    线线交点
  4    tangent         切点（画线到圆）
  5    perpendicular   垂足（画线到另一线）
  6    on_line         线段上最近点
  7    on_circle       圆周上最近点
  8    grid            网格 — 最低，始终存在
```

### 角度吸附例外

角度吸附仅在自由绘制（无吸附或仅网格吸附）时生效。光标在已知几何上时自动禁用——防止闭合操作时终点被扭曲。

---

## 9. 特征树映射模式

### 层次

前后端 — `commands.rs` + `FeatureTree.vue` + `ContextMenu.vue`

### 问题

后端 14 个 `FeatureKind` 变体需在前端统一展示：图标、名称、参数、右键菜单项。不同特征类型的菜单不同——草图形显示"编辑草图"，拉伸形不显示。

### 核心思想

后端 `feature_to_node()` 将变体映射为 `FeatureNode` DTO，`feature_type` 字符串编码类型和来源。前端三处消费：

| 消费点 | 依据 | 输出 |
|--------|------|------|
| `featureIcon()` | `feature_type` 精确匹配 | 唯一 emoji 图标 |
| `isSketchLike` | `feature_type` 前缀/相等 | 右键菜单是否显示编辑/正视 |
| 树形箭头 `└` | `feature_type` 排除规则 | 草图形不显示缩进 |

### 类型字符串编码

```
"Sketch"                    → 📐  编辑菜单 ✓
"SketchModule"              → 🧩  编辑菜单 ✓
"Module:echi.gear:spur_gear" → ⚙️  编辑菜单 ✓
"Extrude"                   → ⬆   编辑菜单 ✗
"Boolean"                   → ➖   编辑菜单 ✗
  ...（其他精确匹配）
```

### 相关文件

- `app/src-tauri/src/commands.rs` — `feature_to_node()` 映射
- `app/src/components/FeatureTree.vue` — `featureIcon()` + 树形箭头
- `app/src/components/ContextMenu.vue` — `isSketchLike` 条件渲染
