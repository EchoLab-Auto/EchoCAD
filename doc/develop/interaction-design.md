# EchoCAD 交互设计

> 面向开发者：描述 EchoCAD 交互系统的完整架构，涵盖界面布局、事件管线、状态管理、坐标变换、吸附系统和工具生命周期。

---

## 目录

1. [界面布局](#1-界面布局)
2. [交互架构](#2-交互架构)
3. [事件管线](#3-事件管线)
4. [坐标变换链](#4-坐标变换链)
5. [吸附系统](#5-吸附系统)
6. [工具状态机](#6-工具状态机)
7. [各工具交互流程](#7-各工具交互流程)
8. [预览渲染](#8-预览渲染)
9. [特征树交互](#9-特征树交互)
10. [右键菜单](#10-右键菜单)
11. [拖拽与层级](#11-拖拽与层级)
12. [插件对话框](#12-插件对话框)
13. [键盘快捷键路由](#13-键盘快捷键路由)
14. [撤销/重做](#14-撤销重做)
15. [错误反馈](#15-错误反馈)
16. [新增工具检查清单](#16-新增工具检查清单)

---

## 1. 界面布局

### 1.1 整体结构

```
┌──────────────────────────────────────────────────────────────────┐
│  工具栏 — 第一行                                                  │
│  EchoCAD [文件▾] [编辑▾] │ （工作区切换） [ 📦 实体 ]  [ 📐 草图 ]│ 测量 撤销 重做│
├──────────────────────────────────────────────────────────────────┤
│  工具栏 — 第二行                                                    │
│                   [工具组] │ [平面▾] [偏移] 测量                    │
└──────────────────────────────────────────────────────────────────┘
├──────────────┬────────────────────────────┬──────────────────────┤
│  左侧面板     │       3D 统一视口           │  右侧面板             │
│              │    UnifiedViewport.vue      │                      │
│  ┌─────────┐ │                            │  属性面板              │
│  │ 特征树   │ │   草图实体 + 3D 实体        │  PropertiesPanel      │
│  │ 插件列表  │ │   吸附指示器 + 预览渲染     │                      │
│  └─────────┘ │                            │  拉伸面板              │
│              │                            │  ExtrudePanel         │
│  左侧面板     │                            │                      │
│  通过一级菜单  │                            │  布尔对话框             │
│  切换上述视图  │                            │  BooleanDialog        │
├──────────────┴────────────────────────────┴──────────────────────┤
│  状态栏 (HomeView.vue 内联)                                       │
│  ── 当前工具 · 吸附类型 · 操作提示 ──                               │
└──────────────────────────────────────────────────────────────────┘
```

### 1.2 各区域职责

| 区域 | 组件 | 职责 |
|------|------|------|
| **工具栏** | `Toolbar.vue` | 文件/编辑菜单、工作区切换（实体/草图）、特征入口、工具切换、约束入口、平面选择、测量模式、撤销/重做 |
| **左侧面板** | `FeatureTree.vue`、插件列表、一级菜单切换 | 通过顶部一级菜单在「特征树」和「插件列表」两个视图间切换。特征树支持层级展示、选择、双击编辑、拖拽重排、右键菜单。插件列表展示已注册的生成器入口 |
| **3D 视口** | `UnifiedViewport.vue` | 实体渲染、草图绘制平面、吸附指示器、选择/拖拽、预览渲染、拾边/测量模式 |
| **右侧面板** | `PropertiesPanel.vue` + 各功能面板 | 特征属性、拉伸参数配置、实体属性、约束属性、布尔对话框。面板内容根据当前选中特征类型动态切换 |
| **状态栏** | `HomeView.vue` 内联 | 当前工具名称、吸附类型标签、操作提示文字 |

### 1.3 工作区切换：实体 / 草图

工具栏分为两行：第一行放置文件菜单、工作区切换标签和全局操作；第二行放置当前工作区的工具组和平面选择：

| 标签 | 激活条件 | 显示内容 |
|------|---------|---------|
| 📦 实体 | 非草图编辑时 | 特征按钮组（拉伸/旋转/扫描/圆角/倒角/抽壳/布尔）、阵列按钮组（线性/圆周/镜像）、平面选择 |
| 📐 草图 | 草图编辑时 | 绘图工具组（选择/直线/圆/弧/矩形/样条/椭圆/裁剪/延伸）、约束工具组（水平/竖直/…）、操作组（拉伸/旋转/清空/退出） |

**切换行为**：点击「草图」标签 → 自动新建草图并进入编辑；点击「实体」标签 → 退出当前草图编辑。

### 1.4 草图编辑模式下的视口行为

| 当前工具 | 视口旋转 | 鼠标操作 |
|---------|:------:|------|
| 选择 (select) | 允许 | 左键选择实体 / 拖拽点 |
| 绘图工具 | **锁定** | 用于绘制（单击放置点、移动预览） |
| 裁剪/延伸 | 允许 | 单击执行操作 |

视口自动对齐到当前草图平面。进入编辑时 `OrbitControls` 根据工具切换自动启用/禁用。

---

## 2. 交互架构

### 2.1 分层结构

```
┌──────────────────────────────────────────────┐
│  Vue 模板层                                   │
│  mouse/pointer/keyboard DOM 事件              │
├──────────────────────────────────────────────┤
│  UnifiedViewport.vue  ← 3D 鼠标事件统一入口   │
│    ├─ getSketchPoint()   屏幕→草图坐标        │
│    ├─ computeSnap()      吸附候选项收集       │
│    ├─ handleDrawingMouseDown()  工具状态机    │
│    └─ onMouseMove()      预览 + 吸附更新      │
├──────────────────────────────────────────────┤
│  Composable 层（共享状态 + 工具函数）          │
│  useSketchInteraction  useSketchGeom          │
│  useThreeScene          useSketchActions      │
│  useFeatureActions                             │
├──────────────────────────────────────────────┤
│  Tauri IPC 层                                 │
│  sketch.ts (TypeScript)  ←→  commands.rs (Rust)│
├──────────────────────────────────────────────┤
│  领域层                                       │
│  echi-core  echi-geom  echi-render  …        │
└──────────────────────────────────────────────┘
```

### 2.2 核心设计决策

- **UnifiedViewport 是单一大入口**：所有 3D 交互的鼠标事件由此组件接收，通过 `switch(activeTool)` 分发到各工具处理逻辑
- **状态集中在 composable**：`SketchInteractionState`（工具中间状态）、`sketchStore`（Pinia—特征树/实体缓存）、吸附结果、预览状态
- **后端无 UI 状态**：Rust 后端不知道当前工具、选中状态、相机位置——它只接受坐标和命令
- **吸附全在前端计算**：每帧 60fps 不能走 IPC，吸附检测必须纯 TypeScript 完成
- **绘制起点延后创建**：首次点击不立即建 Point 实体，而将坐标暂存——取消时不产生 undo 条目

---

## 3. 事件管线

### 3.1 onMouseDown 分发

```
onMouseDown
  ├── 拾边模式？     → handleEdgePick（悬停高亮 + 点击选中边）
  ├── 测量模式？     → handleMeasurePick（拾取点）
  ├── isDrawing？    → handleDrawingMouseDown → switch(activeTool) 分发
  ├── isSketchMode？ → findClosestEntity → 选中 / 开始拖拽
  └── 3D 模式        → raycaster 拾取 solid face
```

### 3.2 onMouseMove 分发

```
onMouseMove
  ├── 拾边模式？     → 悬停高亮最近边
  ├── 拖拽点？       → movePoint（首次 snapshot，后续 no_snapshot）
  ├── isDrawing？    → computeSnap → 更新 activeSnap → 预览渲染
  └── trim/extend？  → findClosestEntity → 悬停高亮实体
```

### 3.3 onMouseUp / onRightClick / onDoubleClick

| 事件 | 触发条件 | 行为 |
|------|---------|------|
| mouseUp | 拖拽点中 | 结束拖拽，发射 drag-end 触发视口刷新 |
| rightClick | 样条绘制中 (≥2 点) | 完成样条 |
| rightClick | 其他绘制中 | 取消手势，清理孤儿点 |
| rightClick | 特征树上 | 打开上下文菜单 |
| doubleClick | 样条绘制中 | 完成样条 |
| doubleClick | 直线绘制中 | 取消当前段（不取消链） |
| doubleClick | 特征树上 | 进入草图编辑（草图形特征） |

---

## 4. 坐标变换链

### 4.1 五层变换

```
屏幕像素 (sx, sy)
  → NDC (-1..+1)
  → THREE.Raycaster 从相机通过 NDC 点
  → 射线与草图平面求交 → 3D 世界坐标
  → 投影到平面的 (u, v) 基底 → 2D 草图坐标 { x, y }
```

### 4.2 平面帧定义

每个 Sketch / SketchModule 特征内联存储其 `PlaneDefinition`：

| 平面 | 原点 | 法线 | uDir | vDir |
|------|------|------|------|------|
| XY | (0,0,0) | (0,0,1) | (1,0,0) | (0,1,0) |
| YZ | (0,0,0) | (1,0,0) | (0,1,0) | (0,0,1) |
| ZX | (0,0,0) | (0,1,0) | (0,0,1) | (1,0,0) |
| Offset | 基准面偏移 | 继承基准面 | 继承基准面 | 继承基准面 |

### 4.3 三个坐标空间的转换函数

| 方向 | 函数 | 用途 |
|------|------|------|
| 草图→世界 | `sketchToWorld3D(sx, sy, plane)` | 预览渲染 |
| 世界→草图 | `worldToSketch2D(point, plane)` | 鼠标事件 → 绘制坐标 |
| 世界→屏幕 | `worldToScreen(x, y, z)` | 吸附检测（像素阈值） |

---

## 5. 吸附系统

### 5.1 吸附类型与优先级

```
优先级  吸附类型        触发条件                    检测方式
  0    endpoint        光标在 Point 实体附近         屏幕像素距离 < SNAP_PX(12px)
  1    center          光标在 Circle/Arc 圆心附近    同上
  2    midpoint        光标在 Line 中点附近          同上
  3    intersection    两条 Line 的交点               线线求交 + 屏幕距离
  4    tangent         画线时，起点到圆的切点          解析几何求切点 + 屏幕距离
  5    perpendicular   画线时，起点到线的垂足          解析几何求垂足 + 屏幕距离
  6    on_line         光标在线段上的最近点            点到线段投影 + 屏幕距离
  7    on_circle       光标在圆弧上的最近点            点到圆/弧投影 + 屏幕距离
  8    grid            整数坐标网格点                 坐标圆整 + 屏幕距离（阈值×60%）
```

### 5.2 吸附收集 → 排序 → 返回

1. 遍历 `store.entities`，将每个候选项的草图坐标转为屏幕坐标
2. 与鼠标屏幕坐标比较，距离 < 阈值则加入候选项列表
3. 按优先级数值排序，同级按距离排序
4. 返回最优候选项（或 null）

### 5.3 角度吸附例外

角度吸附（将线段方向吸附到 0°/45°/90° 等）仅在**自由绘制**时启用。当光标已吸附到已知几何（端点、线、圆等）时自动禁用——防止闭合操作时终点被扭曲。

---

## 6. 工具状态机

### 6.1 统一状态结构

所有工具共享一个 `SketchInteractionState`，位于 `useSketchInteraction.ts` 中。每个工具只使用自己的专属字段，切换工具时通过 `resetDrawingState()` 统一清空。

工具专属字段：

| 工具 | 状态字段 | 用途 |
|------|---------|------|
| 直线 | `lineStartId`, `lineStartPos`, `lineChainOriginId` | 起点追踪 + 链首闭环检测 |
| 圆 | `circleCenterId`, `circleCenterPos` | 圆心延后创建 |
| 弧 | `arcCenterId`, `arcCenterPos`, `arcRadiusPoint`, `arcRadiusSet` | 三步绘制状态 |
| 矩形 | `rectStart` | 第一个角点 |
| 样条 | `splinePoints` | 控制点列表 |
| 椭圆 | `ellipseCenterId`, `ellipseCenterPos` | 中心延后创建 |
| 全部 | `pendingPointIds`, `activeSnap`, `isDragging` | 通用 |

### 6.2 延后创建模式

直线、圆、弧、椭圆的起点在首次点击时不立即创建 Point 实体——坐标暂存在 `*Pos` 字段中。真正的 `addPoint` 调用在第二次点击时发生。如果用户在第二次点击前取消（Esc/右键/切换工具），调用 `deleteEntityNoSnapshot` 清理已创建的点。

**原因**：如果首次点击就建 Point，取消时需要 `deleteEntity`——这会产生一个 undo 条目。延后创建实现了"取消零副作用"。

### 6.3 工具切换清理

```
watch(activeTool 变化)
  → cleanupPendingPoints()      删孤儿点（无 undo 快照）
  → resetDrawingState()         清空所有中间状态
  → clearSketchPreviews()       移除预览几何体
```

---

## 7. 各工具交互流程

### 7.1 直线 (L) — 连续链式 + 闭环检测

```
click₁ → 记录起点（已有实体或延后坐标）+ 链首点 ID
  │
click₂₊ → addLine(prevEnd, currentPos) → prevEnd = currentPos … 链式延续
  │
click→origin → hit === lineChainOriginId → 闭合线段 → 自动终止链
```

- 取消：Esc / 右键 → 删孤儿点 → 重置状态
- 角度吸附：仅在自由绘制时启用（光标不在已有点/线上）

### 7.2 圆 (C) — 两步

```
click₁ → 记录圆心（已有实体或延后坐标）
move   → 预览完整圆
click₂ → 创建圆心（若延后）+ 创建圆
```

### 7.3 弧 (A) — 三步

```
click₁ → 记录圆心
move   → 预览完整圆
click₂ → 确认半径 + 起始角
move   → 预览弧线
click₃ → 创建圆心（若延后）+ 创建弧
```

### 7.4 矩形 (R) — 两步 + 自动约束

```
click₁ → 记录第一角点
move   → 预览矩形 + 尺寸标注
click₂ → 创建 4 角点 + 4 条线 → 自动添加 8 个约束（水平×2、竖直×2、平行×2、等长×2）
```

### 7.5 样条 (B) — 多点

```
click₁₊ → addPoint → 加入 splinePoints
右键/双击 → addSpline(splinePoints)
```

最少 2 个控制点。

### 7.6 裁剪 (T) — 单次点击

```
click → findClosestEntity → trimEntity(id, clickX, clickY)
     → 后端找最近交点 → 保留点击侧部分 → 刷新草图
```

### 7.7 延伸 (X) — 单次点击

```
click → findClosestEntity → extendEntity(id, clickX, clickY)
     → 后端沿方向延伸至边界交点 → 刷新草图
```

---

## 8. 预览渲染

### 8.1 草图绘制预览

每种绘制工具在鼠标移动时渲染临时线框预览：

| 工具 | 预览内容 |
|------|---------|
| 直线 | 从起点到吸附后终点的线段 |
| 圆 | 从圆心到吸附点半径的完整圆 |
| 弧 | 第二步显示完整圆，第三步显示弧线 |
| 矩形 | 对角矩形 + 宽×高标注 |
| 样条 | 控制多边形 + 到光标的临时线段 |

所有预览使用 `sketchToWorld3D` 将草图坐标转为世界坐标，渲染到独立的 `previewGroup` 中。每次 `onMouseMove` 先调 `clearSketchPreviews()` 再重建。

### 8.2 拉伸预览

拉伸预览是真实的 3D 半透明网格：

- 每次参数调整触发 `previewExtrude()` Tauri 命令
- 返回 `RenderMesh` → 构建 `BufferGeometry` → 橙色半透明 `MeshPhongMaterial`（opacity 0.55）
- 120ms 防抖避免高频调用
- 取消/确认时 `dispose()` geometry + material

---

## 9. 特征树交互

### 9.1 树节点构建

从 `features[]` 扁平列表构建层级树：

```
遍历 features，过滤出 parent_id === null 的根节点
  → 递归插入子节点（parent_id === 父节点.id）
  → 每级 depth + 1 → 左侧缩进 16px/级
```

### 9.2 三种选择状态

| 状态 | 字段 | 触发 | 视觉效果 |
|------|------|------|---------|
| 选中 | `selectedFeatureId` | 单击 | 蓝色外框 |
| 活动 | `activeFeatureId` | 单击（同选中） | 蓝色背景 |
| 编辑中 | `editingSketchId` | 双击/右键菜单 | 视口切平面 + 启用绘制工具 |

**关键规则**：三个状态独立存储，不互相覆盖。单击只设选中+活动，不进入编辑。

### 9.3 树节点样式

| 状态 | 样式 |
|------|------|
| 正常 | 透明背景 |
| 悬停 | `#3c3c3c` 灰色背景 |
| 活动 | `#007acc` 蓝色背景 |
| 选中 | `1px solid #4fc3f7` 蓝色外框 |
| 抑制 | opacity 0.45 + 斜体 |
| 错误 | `#4a2020` 红色背景 |
| 拖拽目标 | `2px dashed #4fc3f7` 蓝色虚线框 |

### 9.4 特殊显示规则

- **草图形特征不显示树形箭头**：`Sketch`、`SketchModule`、`Module:*`、`Custom:*` 前缀不渲染 `└` 符号——它们内联携带数据，不是消费型特征
- **插件生成器图标**：`Module:` 前缀的类型字符串通过关键词匹配显示特殊图标（gear → ⚙️, spring → 🌀）
- **抑制按钮**：`●` 活跃 / `◌` 抑制

---

## 10. 右键菜单

### 10.1 菜单项按特征类型条件显示

```
isSketchLike = feature_type === "Sketch"
            || feature_type === "SketchModule"
            || feature_type.startsWith("Module:")
            || feature_type.startsWith("Custom:")
```

| 菜单项 | 草图形 | 非草图形 | 说明 |
|--------|:---:|:---:|------|
| 编辑草图 | ✓ | — | 进入草图编辑 |
| 正视于草图 | ✓ | — | 相机对齐草图平面 |
| 重命名 | ✓ | ✓ | prompt 输入新名 |
| 抑制/取消抑制 | ✓ | ✓ | 跳过/恢复再生 |
| 删除 | ✓ | ✓ | 有依赖时级联确认 |

### 10.2 级联删除确认流程

```
deleteFeature(id)
  ├── 后端返回 dependents + children 列表
  ├── 前端弹出对话框："以下特征将被一并删除：xxx, yyy"
  ├── 用户确认 → deleteFeature(id, cascade:true)
  └── 用户取消 → 不删除
```

---

## 11. 拖拽与层级

### 11.1 拖拽流程

```
dragStart → 记录被拖拽特征的 ID
  │
dragOver  → 检查目标是否为草图形 → 是则高亮（蓝色虚线框）
  │
dragLeave → 取消高亮
  │
drop      → reparentFeature(childId, parentId)
              ├── 后端 Document::reparent_to()
              │     ├── 防循环（is_descendant_of 检查）
              │     └── 移动顺序（子特征移到父后）
              └── 前端 loadState() → 刷新树
```

### 11.2 约束

- **只有草图形可接受子节点**：`Sketch`、`SketchModule`、`Module:*`、`Custom:*`
- **防自拖**：`childId !== parentId`
- **防循环**：后端检测 `is_descendant_of(parentId, childId)`
- **级联删除**：删除父特征时子特征自动移除

---

## 12. 插件对话框

### 12.1 组件

```
PluginDialog.vue
  ├── 生成器名称 + 描述
  ├── 参数表单：每个 ParamDef → 范围滑块 + 数字输入
  ├── 生成按钮（loading 状态）
  └── 关闭按钮 / 遮罩
```

### 12.2 流程

```
用户点击插件列表 → openPluginDialog(generator)
  → PluginDialog 弹窗（参数默认值来自 ParamDef.defaultValue）
  → 用户调整参数
  → 点击「生成」
  → generatePluginFeature(plugin_id, generator_id, params)
     → 后端调 PluginRegistry.generate_sketch()
     → 创建 FeatureKind::SketchModule { sketch, source }
     → loadState() → refreshViewport()
  → 关闭弹窗，toast 提示
```

### 12.3 状态管理

- `activeDialog`：当前打开的生成器信息（null = 关闭）
- `dialogValues`：用户调整的参数值
- `dialogLoading`：生成中禁用按钮
- 关闭后再打开恢复上次的参数值

---

## 13. 键盘快捷键路由

### 13.1 路由优先级

```
1. 输入框/文本域 → 不拦截
2. Ctrl+组合键 → 文件/编辑操作（不区分模式）
3. 非草图模式快捷键（Space, Delete特征, E/W/F/N）
4. 草图模式快捷键（工具切换 L/C/A/R/B/I/T/X, 约束 H/V/P/D/S, Esc, Delete实体）
```

### 13.2 完整映射

| 范围 | 键 | 功能 |
|------|----|------|
| 全局 | `Ctrl+S/O/N` | 保存/打开/新建 |
| 全局 | `Ctrl+Z/Y` | 撤销/重做 |
| 全局 | `Ctrl+0` / `Space` | 适配视图 |
| 全局 | `E` / `W` / `F` / `N` | 拉伸/旋转/圆角/新草图 |
| 草图 | `L/C/A/R/B/I` | 直线/圆/弧/矩形/样条/椭圆 |
| 草图 | `T` / `X` | 裁剪/延伸 |
| 草图 | `S` | 求解约束 |
| 草图 | `H/V/P/D` | 水平/竖直/平行/距离约束 |
| 草图 | `Esc` (×1) | 切换到选择工具 |
| 草图 | `Esc` (×2) | 退出草图编辑 |
| 草图 | `Delete` / `Backspace` | 删除选中实体 |
| 3D | `Delete` / `Backspace` | 删除选中特征 |

---

## 14. 撤销/重做

### 14.1 快照触发规则

| 操作 | 是否快照 | 说明 |
|------|:------:|------|
| 添加/删除/修改实体 | ✓ | `snapshot_if_sketch()` |
| 添加/删除/抑制特征 | ✓ | 命令自带 snapshot |
| 修改参数值 | ✓ | `update_parameter()` |
| 拖拽点（首次 mousedown） | ✓ | `movePoint()` |
| 拖拽点（后续 mousemove） | ✗ | `movePointNoSnapshot()` |
| 切换工具 | ✗ | 纯 UI 状态 |
| 相机移动 | ✗ | 纯视图状态 |

### 14.2 拖拽优化

首次 mousedown 调用 `movePoint`（含 snapshot），后续 mousemove 调用 `movePointNoSnapshot`。如果每次 mousemove 都 snapshot，undo 栈会被一次拖拽填满。

### 14.3 撤销/重做后的清理

撤销/重做替换整个文档 → 所有按 FeatureId 键控的状态（选中、编辑中、颜色）必须重置，因为旧 id 空间与新快照无关。

---

## 15. 错误反馈

### 15.1 三级反馈通道

| 级别 | 通道 | 用途 | 生命周期 |
|------|------|------|---------|
| 即时 | Toast | 操作结果反馈（成功/失败/提示） | 3 秒自动消失 |
| 持久 | `FeatureNode.errors` | 再生失败的持久错误 | 直到下次再生成功 |
| 调试 | `console` | 开发期完整错误栈 | 开发环境 |

### 15.2 Toast 语义

| 类型 | 颜色 | 用途 |
|------|:----:|------|
| `success` | 绿 | "拉伸特征已创建" |
| `info` | 蓝 | "请先选择一个草图" |
| `error` | 红 | "拉伸失败: {详情}" |

### 15.3 再生错误可见性

```
FeatureNode.errors = "extrude produced no mesh"
  → 特征树：⚠ 图标 + 红色背景 + hover tooltip
  → 属性面板：红色错误框显示完整信息
  → 侧栏顶部：错误计数徽章 "N 错误"
```

---

## 16. 新增工具检查清单

添加新的草图工具需要修改：

| # | 文件 | 修改 |
|---|------|------|
| 1 | `stores/sketch.ts` | `Tool` 类型联合添加新值 |
| 2 | `composables/useSketchInteraction.ts` | 状态字段 + 初始化 + 重置 + 标签 |
| 3 | `components/Toolbar.vue` | `sketchTools` 数组添加项 |
| 4 | `components/UnifiedViewport.vue` | `handleDrawingMouseDown` case + `onMouseMove` 预览 + 取消逻辑 |
| 5 | `views/HomeView.vue` | 键盘快捷键 + 状态栏文本 |

单次点击工具额外需要：
- `isDrawing` computed 中排除
- 工具切换时正确清理

需要后端支持时：
- `commands.rs` → Tauri 命令
- `main.rs` → 注册
- `commands/sketch.ts` → TypeScript 封装
- `commands/agent-api.ts` → Agent API（原则 #14）
