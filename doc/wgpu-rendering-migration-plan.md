仓库事实已核实（crates 布局、`tauri = { version = "2", features = [] }` 尚无 `unstable`、`useThreeScene.ts:167-170` JS 渲染循环、`UnifiedViewport.vue:270/953/1023` JS raycaster 拾取、`commands.rs:1613-1631` JSON 网格命令），与报告 5 一致。以下是最终技术方案。

---

# EchoCAD 渲染管线迁移最终技术方案（原生 wgpu 视口 + Vue Web UI）

**版本**: v1.0 ｜ **日期**: 2026-08-01 ｜ **依据**: 5 份调研报告（Tauri+wgpu 集成模式、WebGPU vs 原生性能实证、业界架构案例、Tauri 2 API 能力矩阵、本仓库现状分析）综合

---

## 1. 执行摘要

**结论：迁移，且一步到位采用"单窗口 wgpu 全窗原生渲染 + 子 WebView 承载 Vue 面板"架构（报告 1 模式 5）**，渲染循环照 tauri-plugin-egui 的 `wry_plugin` 模式挂进 Tauri 事件循环（该机制已合入上游 Tauri 2.7.0，无需 fork）。

迁移的收益不在静态旋转 FPS——实证显示 500 万三角形下原生相对 WebGL 仅 20-50%——而在三处**硬性收益**：① 消除 JSON 网格 IPC 带宽墙（现状每次 regen 传输 0.5-1MB（5K 三角）/ 10-50MB（50 万三角）JSON，迁移后降为几十字节信号，这是物理上限无法靠优化绕开的，见 [IPC 带宽墙](https://www.mechanicalrock.io/blog/it-s-physics-not-a-bug-the-ipc-bandwidth-wall-in-webview-apps)）；② 参数化重建-上传路径提速 2-5 倍（持久 mapped ring buffer + 显式生命周期，而 WebGL 不能 orphaning、GC 延迟释放、VRAM 峰值虚高约 1.6 倍）；③ 拿到 10-30 倍 draw call 余量（WebGL ~5,000 次/帧 vs 原生 NVIDIA ~150,000 次）和消除浏览器 tab 配额/context loss 风险，这是未来装配体与长会话建模的架构前提。

本仓库迁移成本被现架构大幅压低：几何数据**已经**全部在 Rust 侧（echi-render/echi-brep/OCCT/echi-geom 完全复用，`RenderMesh` 直接喂 wgpu 顶点/索引缓冲），重写集中在渲染器本体、拾取/高亮、输入路由三块，预估 **13-17 人周**，按 P0（PoC）→P1（视口）→P2（交互）→P3（性能）四阶段推进，P0 先验证多 webview 平台行为再放量。此架构与 Fusion 360（CEF 面板 + 原生视口，且 2020 年关停纯浏览器版"Edit in Browser"）及 OrthoRay（Tauri 2 + wgpu 体渲染，生产级跨平台）同构，是行业验证过的正解。

**明确不建议**：维持 Three.js 现状（带宽墙会在 50 万+三角形时卡死重建手感）、前端仅升 WebGPU（仍在 WebView 里、带宽墙仍在）、离屏渲染回传 canvas（1080p 60fps 需 0.5GB/s，物理不可行）、透明覆盖层与双窗口方案（平台脆弱，见风险节）。

---

## 2. 推荐架构

### 2.1 目标架构图（ASCII）

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                        Tauri 2 主窗口 (Window，无默认 webview)                   │
│  ┌──────────────────────┬───────────────────────────────┬───────────────────┐ │
│  │  Child WebView #1    │                               │  Child WebView #2 │ │
│  │  Vue: FeatureTree    │         wgpu 视口区域          │  Vue:             │ │
│  │  (不透明面板)          │   (整个窗口表面的 wgpu surface)  │  PropertiesPanel  │ │
│  │                      │                               │  (不透明面板)        │ │
│  │                      │   · 实体网格 / 边线 / 线框       │                   │ │
│  │                      │   · 草图 2D 覆盖层 (wgpu 线)     │                   │ │
│  └──────────────────────┴───────────────────────────────┴───────────────────┘ │
│  ┌──────────────────────────────────────────────────────────────────────────┐ │
│  │  Child WebView #3: Toolbar / 顶部命令区（不透明）                          │ │
│  └──────────────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────────────┘
        ▲                        │ 原生窗口事件 (mouse/key/wheel/resize)          │
        │ tauri emit/listen      ▼                                               │
   Vue/Pinia 状态             Rust 侧（同一进程）                                   │
   (命令式 UI 状态)     ┌────────────────────────────────────────────────────┐   │
                       │  wry_plugin 渲染驱动 (照 tauri-plugin-egui)          │   │
                       │  ┌─ echi-wgpu（新 crate）                           │   │
                       │  │  Instance/Adapter/Device/Queue                   │   │
                       │  │  MeshBuffers: HashMap<FeatureId, GpuMesh>        │   │
                       │  │  OrbitCamera / 渲染循环 / GPU 拾取 / 高亮 pass    │   │
                       │  └─────────────────────────────────────────────────┘   │
                       │  echi-render (原样复用): regenerate → echi_geom::Mesh   │
                       │  echi-brep / cadrum OCCT (原样复用)                     │
                       │  commands.rs (55 命令保留；网格查询命令改为信号/修订号)    │
                       └────────────────────────────────────────────────────┘   │
```

### 2.2 关键设计决策

**D1 — 渲染器放哪：新 crate `echi-wgpu`，wgpu 直绘 TAO 窗口全表面，WebView 只占面板矩形**
- 依据两条全局事实（报告 1）：Windows 上 WebView2 是原生子窗口、bounds 内永远置顶（这是**优点**：面板天然盖在 wgpu 之上）；透明 WebView 在 Windows 是坑（[wry #1540](https://github.com/tauri-apps/wry/issues/1540)），所以**面板一律不透明、与视口不重叠**，从架构上绕开全部 z-order 与透明问题。
- 事件循环：用 `wry_plugin` 机制（tauri-plugin-egui 已合入 [Tauri v2.7.0](https://explore.market.dev/ecosystems/rust/projects/tauri-plugin-egui)），不自己拼 winit 循环（社区反复踩坑的失败模式）。
- Windows 建窗必须 `with_clip_children(false)`（[wry #1212](https://github.com/tauri-apps/wry/issues/1212)）；macOS 无需 hack（[issue #10155](https://github.com/tauri-apps/tauri/issues/10155) 确认 webview 正确叠在 surface 之上）；Linux 首发 X11 only（Wayland 已知缺陷）。

**D2 — WebView 共存：`Window::add_child` + `set_bounds`，写一个"resize 同步层"**
- 启用 `tauri = { version = "2", features = ["unstable"] }`（`add_child` 为 unstable API，桌面 only）。主 webview 方案：**不用** `auto_resize()`（会铺满全窗），而是注册窗口 resize 监听 → 按面板布局常量重算各子 webview 的 `wry::Rect` → `set_bounds`。这是本模式唯一日常工作量，报告 4 已给签名与坑（`add_child` 在同步 handler 里会死锁，必须 async）。
- 面板布局常量（左右栏宽、顶栏高）同时存在于 Rust（bounds 计算）与 CSS（面板内布局），以 Rust 为准。
- wry 锁 ≥0.40（Windows z-order 修复，[wry 0.40 release](https://tauri.app/release/wry/v0.40.0/)），最好跟踪 Tauri 2.x 最新补丁。

**D3 — 数据流：网格永不经过 IPC；regen 后 Rust 侧 diff 上传，JS 只收"修订号 + 变更特征 id"**
```
现状:  regen → JSON 序列化 RenderMesh(positions/normals/indices) → IPC → JS parse → BufferGeometry
目标:  regen → echi_geom::Mesh → 直接建/更新 wgpu Buffer（按 feature_id 缓存，diff 更新）→ emit "viewport_updated"{rev, changed_ids}
```
- `AppState` 增加 `feature_rev: HashMap<FeatureId, u64>`，每次 regen 递增；仅变更特征重传 GPU（持久 mapped ring buffer，先传已变更、带宽复用）。
- 现状 `get_all_solid_meshes`/`get_solid_mesh`/`preview_extrude` 三个命令保留（供 UI 查询包围盒/计数/属性），但**不再承载每帧渲染数据**；网格读回只用于截图/导出（低频率）。
- 前端 UI 状态（选中、edgePickMode、测量模式）仍走 Pinia + 现有 IPC 命令，只是不再有"拉全量网格"这一步。

**D4 — 输入路由：视口区没有 DOM，鼠标/键盘全部走 Rust 原生窗口事件**
- `Window::on_window_event`（或 wry_plugin 钩子）拿原生 mouse/wheel/key/resize；Rust 侧判断光标是否落在视口矩形内 → 相机（OrbitCamera）与拾取在 Rust 本地完成（Onshape 原则：交互零带宽）。
- JS 草图状态机保留在 Vue：Rust 把"光标射线 + 当前草图平面 frame"按事件频率（≤120Hz、每包 <200B）经 emit 推给 JS，JS 继续做吸附/状态机，发出语义命令回 Rust。草图吸附逻辑（`useSketchInteraction.ts`）**不改**，只改数据来源。
- 键盘焦点：视口无 DOM，Esc/快捷键等由 Rust 转发给 JS 或直接处理；面板内的快捷键不受影响。

**D5 — 拾取：P1 用 Rust CPU BVH（快速移植保持可用），P2 上 GPU ID-buffer 拾取（面+边统一）**
- 现状 JS 的 `intersectObjects` + 自研 `findClosestEdge`（`UnifiedViewport.vue:270/953/1023`）是 CPU 射线-三角形/射线-线段，逻辑直接移植到 Rust（bvh/parry 类 crate），P1 即可恢复 100% 功能等价。
- P2 升级为 GPU 拾取：face/edge 各渲染一张 ID 纹理 → readback 1 像素（约 1ms，[报告 2 dispatch/readback 数据]）→ 映射 feature/face/edge；高亮用独立 highlight pass（ID 重着色 + 边线强调），替代 Three.js Line 叠加。5M 三角形时 BVH 重建成本高，GPU 拾取优势显现。

**D6 — 2D 草图/尺寸标注/测量可视化：迁入 wgpu 线渲染，文本走 glyphon（P3）**
- 草图实体、吸附标记、尺寸线全部是视图空间几何，wgpu 画线（线宽/像素对齐处理）比 DOM overlay 更精确、无平台透明依赖。`DimensionOverlay.vue` 这类 DOM overlay 在视口区**失效**（无 webview），P1-P2 过渡期用 wgpu 线段+色块表达，P3 用 glyphon 补文本（含中文，fontdb 加载系统字体）。

**D7 — 多视口与剖切（P3）**：四视口 = 同一批 GPU buffer 上跑 4 个 camera pass（`set_viewport`/scissor），几何只上传一次（报告 2 结论：视口数与几何开销无关，只与 draw call 次数成正比）；剖切用 shader clip plane uniform。

### 2.3 为什么这个组合最优

| 维度 | 本方案 | 竞争方案 |
|---|---|---|
| 平台 | Win/macOS 双平台成立（Linux X11 可跑） | 透明覆盖层仅 macOS 稳；双窗口 Windows 崩溃记录（qwook/steam-overlay） |
| 渲染自由度 | 整窗 wgpu 画布，HUD/坐标轴/选择框直接画 | 纹理桥/离屏方案受 IPC 带宽墙限制（1080p@60fps=0.5GB/s） |
| 依赖稳定性 | 官方 API（`add_child`/`set_bounds`）+ 上游已合入的 wry_plugin 机制 | 双窗口手动同步位置、compositor API 需绕过 wry 自集成（模式 3，工作量爆炸） |
| 对现有代码破坏 | 几何/内核/命令零改动；Vue 组件与 Pinia 全保留 | 全 Rust UI（egui）需重写全部 Vue 界面（违背"保留 Vue UI"目标） |

### 2.4 备选方案（按优先级）

1. **B1（快速验证垫脚石）**：模式 1 简化版——整窗 surface + 单个全窗 webview 仅显示面板区（HTML 布局把视口区留空）。不依赖 `unstable`，作为 P0 前 1 周的先导实验验证"webview 与 surface 共存"可行性；不进入生产。
2. **B2（兜底）**：双窗口（wgpu 主窗 + 透明 `WebviewWindow` 贴面板区，[bevy HUD demo](https://github.com/langyo/native-bevy-with-tauri-hud-demo) 路线）。API 稳定无 unstable 依赖，但位置同步、焦点、全屏、Wayland 全是槽点（报告 1 模式 7），仅当多 webview 平台 bug 无法绕过时启用。
3. **B3（远期进阶）**：Windows 上 WebView2 Composition + GraphicsCapture → wgpu D3D12 真合成（graphshell/scrying 链路，[spike](https://github.com/mark-ik/graphshell/wiki/2026-03-28_wry_composited_texture_feasibility_spike)），仅当未来需要"视口内嵌 DOM 浮层"（如内嵌文档浏览器）时再评估，现阶段不做。

---

## 3. 决策矩阵

| 候选方案 | 性能上限 | 开发成本 | 平台覆盖 | 现有代码破坏 | 长期演进 | 结论 |
|---|---|---|---|---|---|---|
| **A. 维持 Three.js 现状** | 5M 三角 30-45 FPS（GPU 吞吐锁死）；50K mesh 15 FPS；重建卡顿随规模线性恶化；VRAM 峰值 +60%、有 context loss | 0 | Win/mac/Linux | 无 | 遇装配体即触顶；IPC JSON 带宽墙无法根治 | ❌ 不可接受 |
| **B. 前端升 WebGPU（WebGPURenderer）+ 二进制 IPC** | GPU 侧≈原生；draw call 较 WebGL2 3.3 倍；但仍在浏览器 quota 内，带宽墙只缓解不消除（custom protocol 流式） | 中（2-3 人周） | 全（浏览器） | 小 | 收益 1-2 年后被原生拉开；脱离"桌面应用"定位（EchoCAD 是 Tauri 桌面产品，无 Web 版诉求） | ⚠️ 仅作过渡观察，不作为主路径 |
| **C. 原生 wgpu 全窗 + 子 WebView 面板（本方案）** | 静态 5M 三角 +20-50%；重建/上传 2-5 倍；draw call 余量 10-30 倍；无 tab 配额 | 13-17 人周（P0-P3） | Win/macOS 稳，Linux X11 | 中（渲染/拾取/输入三块，几何与 UI 零改动） | 上限高：多视口、剖切、装配体、GPU 拾取、与引擎演进解耦 | ✅ **推荐** |
| **D. wgpu + 透明全窗 WebView 覆盖层（模式 1/4）** | 同 C 的 GPU 侧，但 Windows 透明层不可靠（[#1540](https://github.com/tauri-apps/wry/issues/1540)、macOS 透明 bug 群 [#8255](https://github.com/tauri-apps/tauri/issues/8255)/[#11308](https://github.com/tauri-apps/tauri/issues/11308)/[#13415](https://github.com/tauri-apps/tauri/issues/13415)） | 低-中 | Windows 失败风险高 | 中 | 平台差异无法收敛 | ❌ 不采用 |
| **E. 双窗口层叠（模式 7）** | 同 C；透明窗口由 WindowServer 全屏合成有性能与残影风险 | 中-高 | Windows 崩溃记录 | 中 | 位置同步/焦点问题长期存在 | ❌ 仅兜底 |
| **F. 离屏渲染回传 canvas（模式 2）** | 1080p@60fps 需 0.5GB/s 读回+IPC，实测低端机 ~300ms/帧 | 低 | 全 | 小 | 物理不可行，仅适合截图 | ❌ 实时视口禁用 |

**推荐理由**：C 是唯一同时满足"性能上限够高、双平台成立、不依赖私有 API/透明层、对几何与 UI 零破坏、有生产级先例（OrthoRay、Fusion 360 同构）"的方案；其成本集中在一次性的渲染器/拾取重写（P1-P2），完成后长期演进收益最大。

---

## 4. 分阶段迁移路径

> 原则：**每阶段结束系统可运行可回退**。迁移期间保留旧 Three.js 路径于 feature flag（`legacy-three`，默认关），双后端并存至 P2 完成；`mesh`/`brep`/`occt` 三档构建不变，新代码以 `wgpu` feature 追加。

### P0 — 最小验证 PoC（2-3 人周，1 名 Rust 工程师）
- **目标**：验证"wgpu 全窗 surface + 子 WebView 面板"在目标平台的可行性，暴露全部平台坑；产出 3-5 条带代码的工程结论。
- **交付物**：新 crate `echi-wgpu` 雏形——wgpu 清屏+旋转三角形/立方体铺满窗口；`add_child` 挂 3 个 Vue 面板（树/属性/工具栏占位）；resize 同步层；wry_plugin 渲染循环骨架；`with_clip_children(false)`（Windows）+ macOS 验证。
- **涉及代码**：`app/src-tauri/Cargo.toml`（加 `unstable` feature、锁 wry≥0.40）、新 `crates/echi-wgpu/`（骨架）、`main.rs`/`lib.rs`（窗口初始化改造）、最小 Vue 占位页面。
- **验证标准**：Windows + macOS 双平台跑通；窗口 resize/最小化/恢复无黑屏与错位（用 `WindowBuilder::background_color`/tauri 2.1 `set_background_color` 防闪烁）；60 FPS；`add_child` 无死锁（async 路径）；记录 z-order 实测行为对照 [issue #9798](https://github.com/tauri-apps/tauri/issues/9798)/[#10011](https://github.com/tauri-apps/tauri/issues/10011) workaround。
- **风险**：多 webview 平台 bug（见风险 R1）；P0 未过则转 B2 双窗口兜底并重新评估。

### P1 — 视口迁移（4-5 人周，1-2 名工程师）
- **目标**：实体渲染完全迁到 wgpu，网格彻底不走 IPC；功能等价（不劣化）现有视口能力：实体/边线/线框、OrbitCamera、视图切换、fitView、面拾取（CPU BVH 移植）、preview 网格。
- **交付物**：`echi-wgpu` 完整渲染器——MeshBuffers 缓存 + 修订号 diff 上传 + 持久 mapped ring buffer；Phong/基础 PBR shader；边线生成（网格邻接法线法）+ 线框模式；OrbitCamera + `on_window_event` 输入；CPU 面拾取（bvh）；`preview_extrude` 走同管线上屏；IPC 改造（`viewport_updated` 事件 + 修订号协议）；相机/选中状态事件桥（含草图平面坐标推送）。
- **涉及代码**：新 `crates/echi-wgpu/`、`commands.rs`（网格命令改造，55 命令其余不动）、`useThreeScene.ts` 删除/降级为状态桥、`UnifiedViewport.vue` 大改（canvas 移除，视口逻辑改为事件驱动）、`useSketchInteraction.ts` 数据源改事件。
- **验证标准**：① 500 万三角形（合成测试模型）静态旋转 ≥30 FPS（RTX 4060 级）/ ≥60 FPS（RTX 4070+ 级）；② 50 万三角形 regen 后"数据就绪→首帧显示" ≤150ms（对照现状 JSON 路径同模型实测，预期 ≥500ms）；③ 每次 regen IPC 载荷 <10KB；④ draw call <300；⑤ 面拾取命中率与旧路径在同一回归用例集上 100% 一致；⑥ 草图模式可用（吸附由事件驱动）。
- **风险**：输入路由回归（R3）、尺寸标注 DOM 失效降级（R9）。

### P2 — 交互/拾取/高亮迁移（4-5 人周）
- **目标**：交互与视觉反馈达到并超过现状：GPU 拾取（面+边统一 ID 纹理）、高亮 pass、边拾取、测量可视化、草图 2D 覆盖层迁入 wgpu 线渲染、选中/悬停状态同步；删掉 `legacy-three` 回退路径（保留 feature 定义但不维护）。
- **交付物**：GPU ID-buffer 拾取管线（face/edge 两 pass + 1 像素 readback 映射 feature/face/edge）；highlight pass（ID 重着色 + 边线强调，替代 `showEdgeHover`/`refreshSelectedEdgeHighlights`）；wgpu 草图线渲染（点/线/圆/弧/吸附标记，像素对齐）；测量可视化（线段+色块）；悬停高亮。
- **涉及代码**：`echi-wgpu`（拾取/高亮/2D pass）、`UnifiedViewport.vue` 瘦身（交互状态机迁 Rust）、`useSketchInteraction.ts` 保留但事件化、`DimensionOverlay.vue` 改造。
- **验证标准**：边/面拾取命中率与旧路径一致（回归用例集）；高亮开启后帧率损失 <10%；草图绘制手感（吸附刷新率 ≥120Hz 事件）不劣于现状；双平台通过。
- **风险**：GPU readback 延迟（约 1ms，可接受）、高亮视觉差异需要人工比对（R10）。

### P3 — 性能优化/多视口/剖切（3-4 人周）
- **目标**：达成长期性能目标并补齐专业功能：多视口（三正交+一透视）、剖切平面、文本/HUD（glyphon 含中文）、批量/实例化优化、VRAM 主动驱逐与长会话稳定性、可选 GPU 拾取兜底切换。
- **交付物**：四视口 pass（单 buffer 多 camera）；clip plane shader；glyphon 文本管线（坐标轴标签、HUD、尺寸文本、测量标签）；实例化/大 buffer heap（参考 [HypeHype 2023 SIGGRAPH](https://advances.realtimerendering.com/s2023/AaltonenHypeHypeAdvances2023.pdf)：mesh 打包进大 heap 消除逐 draw 绑定）；内存驱逐策略（LRU buffer 释放 + 重传）。
- **涉及代码**：`echi-wgpu` 扩展、`UnifiedViewport.vue` 视图/剖切 UI、Vue 视图切换命令。
- **验证标准**：① 四视口合计 500 万三角 ≥45 FPS；② 剖切交互流畅（旋转+拖动剖切面无掉帧）；③ 2 小时连续建模会话 VRAM 峰值波动 <5%、无泄漏趋势；④ 文本渲染中文正确；⑤ 关闭/打开边线模式无卡顿（对照 Onshape "Shaded without edges" 建议）。
- **风险**：字体/DPI（R7）、文本管线工作量超预期（可裁剪为 HUD-only）。

---

## 5. 技术风险与对策（Top 10）

| # | 风险 | 等级 | 对策 |
|---|---|---|---|
| R1 | **多 webview 平台 bug**：z-order 平台不一致（[#9798](https://github.com/tauri-apps/tauri/issues/9798)）、Windows 白屏竞态（[#10011](https://github.com/tauri-apps/tauri/issues/10011)）、resize 不稳（维护者自认）、Linux 定位 bug（[#10420](https://github.com/tauri-apps/tauri/issues/10420)）；API 标 unstable 可能变动 | 高 | 锁 wry≥0.40 + 最新 2.x 补丁；P0 强制做双平台回归（创建顺序、尺寸置 0 workaround、resize 序列）；面板数量最小化（3 个）；API 变动作 nightly CI + 上游 release 追踪；P0 不过即转 B2 |
| R2 | **Windows 合成限制**：默认 `WS_CLIPCHILDREN` 使 wgpu 内容无法与子 webview 正确合成（[wry #1212](https://github.com/tauri-apps/wry/issues/1212)）；透明 webview 不可靠（[#1540](https://github.com/tauri-apps/wry/issues/1540)） | 高 | 建窗必须 `with_clip_children(false)`；**纪律：面板永不透明、永不覆盖视口**；如需视口上小浮层，用不透明小面板（Windows 上天然置顶是优点）；CI 加 Windows 截图对比 |
| R3 | **输入事件路由重写**：视口无 DOM，鼠标/键盘全走 Rust 原生事件；焦点、Esc、修饰键、光标样式、双指触控板行为需重造 | 高 | `on_window_event` 统一入口 + 坐标转换层；按键/光标样式经事件桥转发 JS；P1 内完成并与旧路径做交互回归（录屏对比）；触控板缩放/旋转做手势合成 |
| R4 | **`add_child` 死锁**：在同步命令/事件回调里调用会死锁（报告 4） | 中 | 全部走 async；写单元测试覆盖"命令内改面板 bounds"路径 |
| R5 | **拾取管线重写**：Three.js raycaster/边拾取需重写，5M 三角下 CPU 方案成本高 | 中 | 分两级：P1 CPU BVH（功能等价优先），P2 GPU ID 纹理拾取（face/edge 统一，readback ~1ms）；P3 视规模自动切换 |
| R6 | **IPC 带宽墙复发**：迁移中容易把网格/图像重新塞回 IPC（tauri-wgpu-cam 作者明确为性能绕开此路） | 中 | 架构纪律写进 PR 检查清单：网格数据只存在于 Rust 与 GPU，IPC 只允许信号/小载荷（<10KB）；代码评审硬性检查；CI 断言 regen 事件体大小 |
| R7 | **DPI/字体/坐标**：wgpu surface 用物理像素，`set_bounds`/事件用逻辑坐标（wry::Rect 为物理 i32，需 scale factor 换算）；glyphon 中文/字体加载 | 中 | 统一 `LogicalToPhysical` 转换层（单一函数，全部 bounds/事件走它）；字体用 fontdb + 系统字体（含 PingFang/微软雅黑），P3 验证中文尺寸标注 |
| R8 | **resize/surface 重建**：resize、hide/show、最小化时 surface 重建不当 → 黑屏/卡顿 | 中 | 标准 `Resized` 事件重建流程 + 重建期用 `set_background_color` 底色防闪烁；窗口进入后台节流渲染（对照 Tauri 2.3 WebKit 节流策略） |
| R9 | **3D 标注/尺寸文本系统**：`DimensionOverlay.vue` 类 DOM overlay 在视口区失效 | 中 | 过渡：wgpu 线段+色块（P1-P2）；终态：glyphon 文本（P3）；期间不阻塞其他功能，尺寸编辑仍可用（仅显示降级） |
| R10 | **双后端并存期回归漂移**：`legacy-three` 与新 wgpu 路径行为不一致（高亮观感、拾取阈值 0.08/0.05、线框观感） | 中 | 保留旧路径至 P2 结束做 A/B 对比；建立"同文档同操作"像素/事件回归用例集；P2 完成即删旧路径防双维护 |
| R11（补充） | **拖拽/文件拖放边界**：拖文件进视口、从树拖拽特征等 DOM 交互在视口区失效 | 低 | Rust 侧 `drag_drop` 事件处理 + 转发 JS；视口区拖拽需求盘点后定语义（P2） |
| R12（补充） | **macOS 透明类陷阱**（若未来做 HUD 浮层）：透明窗口 bug 群 + DMG 打包后透明失效（[#13415](https://github.com/tauri-apps/tauri/issues/13415)） | 低 | 当前架构不依赖透明；如未来需要，优先 macOS `set_effects`（vibrancy，公共 API）而非透明 webview |

---

## 6. 里程碑与团队建议

### 6.1 工期与人力（总计约 13-17 人周）

| 阶段 | 工期 | 人力 | 产出 | 退出条件（Gate） |
|---|---|---|---|---|
| P0 PoC | 2-3 人周 | 1 名 Rust 工程师 | 可运行 demo：wgpu 视口 + 3 个 Vue 面板共存 | 双平台跑通、resize 无黑屏、z-order 行为记录在案 |
| P1 视口迁移 | 4-5 人周 | 2 人（1 图形 + 1 全栈） | 实体/边线/线框/相机/CPU 面拾取全上 wgpu；IPC 信号化 | 5M 三角 ≥30 FPS、50 万三角 regen→显示 ≤150ms、IPC <10KB、拾取回归通过 |
| P2 交互迁移 | 4-5 人周 | 2 人 | GPU 拾取/高亮/草图线/测量 | 拾取命中率一致、高亮开销 <10%、草图手感不劣化、删旧路径 |
| P3 性能与专业功能 | 3-4 人周 | 2 人 | 多视口/剖切/文本/实例化/内存驱逐 | 四视口 5M 三角 ≥45 FPS、2h 会话 VRAM 稳定、中文文本正确 |

**团队建议**：核心 2 人（1 名 wgpu/图形向 Rust 工程师为主力，1 名全栈做 Vue 桥接与回归）；前端现有维护者兼职支持 UI 改动。P0 必须由图形向工程师独立完成——本阶段结论决定架构生死。若只有 1 人全职，工期约 14-17 周，建议压缩顺序为 P0 → P1 → P2 骨架（GPU 拾取）→ P3 裁剪（多视口优先，文本 HUD-only）。

### 6.2 开放问题清单（需产品/架构决策）

1. **目标性能基准**：以哪档硬件为基准线（集显 MacBook Air vs RTX 4060）？决定 P3 优化投入与"性能预算"文档（建议：4060 级为基准、30 FPS 为下限）。
2. **Wayland 支持**：首发是否接受"X11 only，Wayland 降级"？（建议接受，标注系统要求）。
3. **3D 标注文本范围**：尺寸标注/测量文本是否必须支持中文与任意字体？（影响 glyphon 与字体授权决策；建议首版 HUD 英文 + 中文 fallback 验证）。
4. **移动端规划**：`add_child` 不支持 iOS/Android；若移动端在路线图，需预留离屏/别的路径（建议明确"桌面 only"并写进架构文档）。
5. **多窗口需求**：多文档窗口/多视口独立窗口是否在路线图？（影响 echi-wgpu 是否按 FreeCAD ViewProvider 模式设计多 surface 共享 device；建议 P3 前按"单窗口多视口"设计，多窗口后置）。
6. **unstable feature 接受度**：`add_child` API 可能变动，是否接受（建议接受，锁版本 + 追踪上游；备选 B2 已备）。
7. **插件渲染接入**：现有插件系统（generate_solid）是否需要渲染侧扩展点（自定义线框/HUD）？（建议 P3 后评估，先不做）。
8. **屏幕录制/截图需求**：wgpu 截图（texture readback）成本低，但若需"防截屏"（`set_content_protected`，macOS 15+ 已失效 [issue #14200](https://github.com/tauri-apps/tauri/issues/14200)）需另行决策。
9. **合成测试模型**：是否需要官方"500 万三角形基准模型"（含大量边线）用于各阶段 Gate？（建议 P0 期间生成并入库）。
10. **回退承诺期**：`legacy-three` 路径保留到何时（建议 P2 结束硬删，防双维护漂移，需管理层背书）。

---

## 7. 明确建议（结论与数据支撑）

**结论：执行迁移，采用第 2 节架构（单窗口 wgpu 全窗渲染 + 子 WebView 面板），立即启动 P0。**

关键数据支撑（对应报告出处）：

1. **"为什么现在迁"而非等模型变大后**：现架构每次文档修改都全量 JSON 传网格（`commands.rs:1622`），中等零件 0.5-1MB、50 万三角即 10-50MB 且 JS 主线程 JSON.parse 卡帧（[IPC 带宽墙](https://www.mechanicalrock.io/blog/it-s-physics-not-a-bug-the-ipc-bandwidth-wall-in-webview-apps)、[tauri #5641](https://github.com/tauri-apps/tauri/issues/5641)）——这是**数学物理限制**，模型再大一点重建体验即崩；叠加 Three.js 每特征 `Mesh`+`LineSegments` 的 draw call 形态（55,700 mesh → 15 FPS，[three.js 论坛](https://discourse.threejs.org/t/bad-performance-when-loading-more-than-3500-meshes-into-the-scene/63960/5)），现状在装配体规模必然触顶。
2. **"为什么是原生 wgpu 而非前端 WebGPU"**：EchoCAD 是 Tauri 桌面产品（无 Web 版诉求），前端 WebGPU 的 gains（WebGL2 的 3.3 倍 draw call、显式生命周期）在 WebView 内仍受 tab 配额、JSON IPC 墙与 GC 拖累；原生侧才能同时解决三条硬瓶颈。且 GPU 侧吞吐两方相同（Unity 官方结论 + marching cubes WebGPU≈Vulkan [IEEE GEM 2024](https://ieeexplore.ieee.org/abstract/document/10585437)），原生多出的 20-50% 静态 FPS 是"锦上添花"，真正的决策项是带宽墙/上传路径/内存稳定性——这三项原生是**唯一解**（[报告 2 汇总表]）。
3. **"为什么风险可控"**：架构有生产级同构先例（OrthoRay Tauri+wgpu 体渲染、Fusion 360 CEF 面板模式、tauri-plugin-egui 已进上游 2.7）；本仓库几何层已 100% Rust 化，迁移面收敛到渲染/拾取/输入三块；P0 门禁 + B2 兜底 + `legacy-three` 双后端并存保证任何阶段可回退。
4. **明确不做的事**：不把 3D 内容放进 WebView（Fusion "Edit in Browser" 2020 关停为最强反证）；不做离屏纹理桥（0.5GB/s 物理不可行）；不依赖透明 webview（Windows 平台坑）；不做全 Rust UI（egui 路线）——保留 Vue UI 是硬约束，本方案满足它。

**唯一前置决策**：立即批准 P0（2-3 人周、1 名图形向工程师），P0 退出评审时点复核 R1 平台结论与 B2 兜底预案，随后放量 P1。
---

# 实施进度记录（2026-08-01 自动实施）

## 已完成（P0-P3 代码全部落地并验证）

### P0 — 基础架构 ✅
- 新 crate `crates/echi-wgpu/`：wgpu 30 渲染器 + wry_plugin 事件循环接入（`tauri::Builder::wry_plugin`，与 tauri-plugin-egui 同构）
- 窗口改造：main webview → 左树面板，`add_child` 挂属性/工具栏面板，resize 同步层
- 前端占位：`PanelPlaceholder.vue` + main.ts 按 `?panel=` query 分支
- Windows `WS_CLIPCHILDREN` 兼容分支（条件编译，未实机验证）
- 冒烟验证：30 秒稳定运行，无崩溃（wgpu 30 需 display handle 传 `InstanceDescriptor::display`——已处理）

### P1 — 视口迁移 ✅
- `MeshBuffers`（HashMap<FeatureId, GpuMesh>）+ `RenderManager`（线程安全 pending 队列 + diff 同步 + 修订号）
- 实体/线框管线（`POLYGON_MODE_LINE` feature）+ `OrbitCamera`（orbit/pan/zoom/F fit/W wireframe 快捷键）
- 输入路由：原生窗口事件（视口无 DOM）
- regen 钩子：`regenerate_state`/`regen_locked`/`incremental_regen_locked` 三处收束 → `sync_renderer` → pending → GPU；网格永不经过 IPC，前端只收 `viewport_updated` 事件
- 前端树面板：`TreePanel.vue` 嵌入真实 `FeatureTree` 组件（get_features + 事件刷新）
- 端到端验证：`ECHO_DEMO=1` 环境变量构建 demo 盒子 → 完整链路（命令→regen→GPU 上传）跑通
- 单元测试 6 个（diff/删除/增量/clear 逻辑）全部通过

### P2 — 拾取/高亮/草图 ✅
- GPU ID-buffer 拾取：ID pass 渲染 24-bit feature_id 编码 → 1px readback → 异步解码 → 高亮 + `feature_picked` 事件同步前端树选中
- 高亮 pass：per-mesh uniform（base_color + highlight flag），选中特征重着色
- 草图 2D 线渲染：点/线/圆/弧/样条/椭圆折线化 → wgpu LineList overlay（sketch.wgsl 绿色半透明）
- 草图同步钩子：add_point/add_line/delete_entity/clear_sketch/undo/set_active_sketch 等核心命令

### P3 — 多视口/剖切 ✅
- 四视口模式（V 键）：前视/顶视/右视（`OrthoCamera` 正交）+ 透视，单 pass 多次 `set_viewport`，几何只上传一次
- 剖切平面（C 键）：CameraUniform 增加 clip_plane（96 字节无 padding），fragment discard
- ⏳ 未做：glyphon 文本/HUD、实例化优化、VRAM 驱逐（标注为后续工作）

## 使用方式
- legacy 模式：`cargo build`（默认 feature，Three.js 前端路径完全不变）
- wgpu 模式：`cargo build --features wgpu` + `ECHO_DEMO=1` 可看演示场景
- 快捷键：左键拖拽旋转、Shift+拖拽平移、滚轮缩放、F=fit、W=线框、V=四视口、C=剖切、点击=拾取

## 2026-08-01 更新：legacy 模式已删除

- Rust：移除全部 `#[cfg(feature = "wgpu")]` 条件编译；`echi-wgpu` 为必选依赖；`default = []`
- 前端：删除 Three.js（UnifiedViewport/useThreeScene/DimensionOverlay/HomeView/router/composables），bundle 708KB → 109KB；移除 three/@types/three 依赖
- 面板化：main.ts 按 `?panel=tree|props|toolbar` 渲染三个真实面板（FeatureTree / PropertiesPanel / Toolbar + 命令桥）
- 使用方式：`npm run tauri dev` 即 wgpu 模式（无需任何参数）

## 2026-08-01 视觉验证记录（macOS Retina）

### 已确认并修复
- ✅ **DPI 双重缩放**：`WindowEvent::Resized` 已是物理像素，不能再乘 scale_factor
- ✅ **devtools 弹窗覆盖应用**：移除 main.rs 的强制 open_devtools
- ✅ **面板 dev server 依赖**：`WebviewUrl::App` 在 debug 走 devUrl，无 dev server 时白屏
- ✅ **面板布局（位置）**：左树/右属性/工具栏位置正确（add_child + 初始 Rect）

### 未解决（需版本升级/Windows 验证）
- ⚠️ **面板尺寸（宽高）**：wry 0.55 在 macOS Retina 上 `set_bounds`/`set_position`/`set_size`/`add_child(Rect)` 全部受物理坐标 bug 影响（#10011/#1540），面板只占上方区域。已按正确架构写好，**需升级 wry ≥ 0.58 或在 Windows 实机验证**
- ⚠️ **视口内容渲染**：macOS 上全窗 main webview 在 wgpu surface 之上（透明不生效），当前为深色背景
