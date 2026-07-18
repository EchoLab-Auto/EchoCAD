# EchoCAD 前端依赖说明

## ui-frame 组件库

项目计划接入 `https://github.com/EchoLab-Auto/ui-frame.git` 作为平面 UI 组件库。

由于当前环境无法通过 SSH 访问该私有仓库，`package.json` 中暂未声明该依赖。待获得仓库访问权限后，请在 `app/package.json` 的 `dependencies` 中添加：

```json
"echolab-ui-frame": "github:EchoLab-Auto/ui-frame"
```

然后执行 `npm install`。

如果该仓库发布到 npm，也可替换为对应的包名与版本。
