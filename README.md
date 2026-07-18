# EchoCAD

仿 SolidWorks 逻辑的桌面端 3D CAD 软件。

- **后端**：Rust（几何内核、参数化模型、文件 IO、Tauri 命令）
- **前端**：Tauri 2.0 + Vue 3 + TypeScript + Three.js
- **平面 UI 组件**：https://github.com/EchoLab-Auto/ui-frame.git（待接入）

## 快速开始

```bash
# 1. 安装前端依赖
cd app
npm install

# 2. 开发模式（热重载）
npm run tauri dev

# 3. 生产构建
npm run tauri build

# 4. 仅构建前端（验证 TS / Vite）
npm run build

# 5. 运行 Rust 测试
cargo test --workspace
```

## 项目结构

```text
EchoCAD/
├── Cargo.toml              # Rust workspace
├── crates/                 # Rust 核心 crate
│   ├── echi-core/          # 文档模型、特征树、参数
│   ├── echi-geom/          # 2D/3D 几何与约束求解
│   ├── echi-render/        # 渲染数据生成（特征树再生）
│   ├── echi-io/            # 文件序列化与导入导出
│   ├── echi-plugin/        # 插件 trait + 注册中心
│   └── echi-plugin-gear/   # 示例：齿轮生成器插件
├── app/                    # Tauri + Vue3 应用
│   ├── src/                # 前端源码
│   │   ├── components/     # UnifiedViewport / ExtrudePanel / DimensionOverlay
│   │   ├── composables/    # useThreeScene / useSketchInteraction
│   │   ├── stores/         # Pinia store
│   │   ├── commands/       # Tauri 命令包装
│   │   └── views/          # HomeView（主界面）
│   └── src-tauri/          # Tauri Rust 入口（commands.rs）
└── doc/                    # 项目文档
    ├── architecture.md     # 0.2 架构总览
    ├── interaction-guide.md
    └── plan.md
```

详见 [doc/architecture.md](doc/architecture.md) 与 [doc/design-principles.md](doc/design-principles.md)。

## 功能

### 草图（2D）
- 绘制工具：点 / 直线 / 圆 / 弧线 / 矩形 / 样条 / 椭圆
- 智能吸附：端点 / 中点 / 圆心 / 交点 / 网格
- 标准角度吸附（0°/45°/90°...）
- 17 种约束：水平 / 竖直 / 平行 / 垂直 / 相切 / 同心 / 相等 / 中点 /
  固定 / 角度 / 直径 / 距离 / 共点 / 共线 / 对称 / 点在线上 / 半径
- Gauss-Newton + LM 阻尼约束求解（有限差分 Jacobian）
- 驱动尺寸标注（距离 / 半径 / 角度，可点击编辑）

### 特征（3D）
- 拉伸（单侧 / 中平面 / 双向，含拔模角）
- 旋转（自定义轴或默认 Y 轴）
- 扫描（轮廓 + 路径，Frenet 框架）
- 圆角（基于二面角的边检测 + 弧带）
- 倒角（同上，单平面斜角）
- 抽壳（角度加权法向偏移）
- 布尔运算（并集 / 差集 / 交集）
- 阵列（线性 / 圆周）和镜像

### 多环草图
拉伸支持外环 + 任意数量孔洞（如带孔板）。自动按面积识别外/内环，
使用桥接边算法做带孔三角化。

### 特征树管理
- 重命名（双击或右键）
- 抑制 / 取消抑制（即时跳过再生）
- 级联删除（带确认对话框）
- 依赖追踪（删除被引用的特征会警告）
- 错误状态显示（再生失败的特征标红，含错误信息）

### 文件 IO
- 新建 / 打开 / 保存 / 另存为
- 最近打开列表（持久化，最多 10 项）
- 导出 STL（ASCII）
- 导出 OBJ（含顶点法线）

### 撤销 / 重做
基于 JSON 快照的栈，最多 50 步。

### 插件系统
实现 `echi_plugin::Plugin` trait 即可扩展。当前内置：
- `echi-plugin-gear`：渐开线直齿轮生成器（含 inv(α) 补偿、正确齿廓倾斜方向）
- `echi-plugin-cycloidal`：摆线针减速器（摆线轮 + 针轮轮廓，按齿数/减速比、
  针齿中心圆半径、偏心距、针齿半径生成）

## 开发状态

- [x] 项目架构与开发计划
- [x] Tauri 2.0 + Vue3 工程骨架
- [x] 最小可运行应用（工具栏、特征树、3D 视口）
- [x] 2D 草图编辑器（点 / 直线 / 圆 / 弧 / 矩形 / 样条 / 椭圆）
- [x] 约束求解（17 种约束，Gauss-Newton + LM）
- [x] 3D 几何内核与视口
- [x] 参数化特征树（含抑制、级联删除、依赖追踪）
- [x] 多环拉伸（外环 + 孔）
- [x] 边检测圆角 / 倒角（基于二面角）
- [x] 文件 IO（含最近文件、另存为）
- [x] 撤销 / 重做
- [x] 驱动尺寸标注
- [x] 插件系统（gear 示例）

## 注意事项

- `echolab-ui-frame` 为私有仓库，当前未接入。需配置 SSH/Git 访问权限后，
  在 `app/package.json` 中添加依赖并重新安装。
- macOS 生产构建的 `.dmg` 打包需要系统工具支持，`.app` 包本身已可正常生成。
- 当前布尔差集 / 交集为视觉近似（合并 + 翻转法向），不是真正的 CSG；
  未来计划接入 BSP / 八叉树算法。

## 开发计划

详见 [doc/plan.md](doc/plan.md)。
