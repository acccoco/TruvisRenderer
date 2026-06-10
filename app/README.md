# app

`app/` 是项目的应用域目录，放置 app 层公共组件、主体 Truvis app 和独立 samples。这里的 crate 依赖引擎层能力，但不向 `engine/` 反向注入业务代码。

## 目录职责

- `app-kit/`：公共 app 组件，包括 GUI、输入/相机控制、overlay 与 RT pipeline glue。
- `truvis/`：主体 app crate，提供 `truvis-app`，默认加载 Sponza 并叠加程序化材质测试 cubes。
- `samples/hello-triangle/`：Triangle 示例，提供 `triangle`。
- `samples/shader-toy/`：ShaderToy 示例，提供 `shader-toy`。
- `samples/cornell/`：Cornell Box 光追示例，提供 `rt-cornell`。

## 边界约束

- `app-kit` 只放可复用组件，不放具体 app state。
- sample 专用 pass 留在对应 sample crate 内。
- 平台窗口和事件循环由 `engine/app-frame/truvis-winit-app` 提供，app crate 只注入 `RenderAppShell<ConcreteApp>`。
