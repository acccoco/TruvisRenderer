# Workspace scripts

`scripts/` contains offline tooling only. These scripts read source scenes or
build metadata and produce conversion/build outputs; runtime Rust crates do not
depend on them.

## Scene conversion

`scene/export_gltf.py` accepts a `.blend` or `.fbx` source and writes a separate
glTF scene with external textures. Python 3 and Blender are required; Pillow is
required when the source texture root contains images that must be staged or
converted:

```text
python scripts/scene/export_gltf.py \
  --source <scene.blend|scene.fbx> \
  --output-dir <output-directory> \
  --blender <blender-executable>
```

The output contains `scene.gltf` and `textures/`. The source scene and source
textures are read-only. Blender is required only during conversion; the runtime
loads the resulting glTF and external images through the existing asset path.

Conversion uses a fresh temporary directory for each run. Blender output is
checked for external resources and references before publication; resources are
replaced first and `scene.gltf` last. Blender/staging/validation failures preserve
the previous export. Publication is not an atomic transaction across all files:
an I/O failure or interruption during replacement can leave mixed versions.
Unrelated and obsolete files in the output directory are not removed.

The output must not contain the source scene or any Blender source image, and
must not overlap the source texture root. Staging rejects case-insensitive stem
collisions (for example `color.jpg` and `color.png`) instead of choosing one.
Generated bake filenames use controlled numeric names, independent of material
names. Texture size must be positive. The success message names the final output
only after publication.

程序 Base Color 的烘焙仅处理当前 scene mesh 槽位引用的材质，要求输出直接连接 Principled
BSDF，且 Cycles/Eevee 使用相同输出；需要烘焙的 mesh 仍限一个材质槽。每个对象使用独立
材质和烘焙图，必要时复制共享 mesh，代价是增加转换后的材质与贴图。未被槽位引用的材质不参与。
原 UV 和 render UV 保留；新增 UV 只用于烘焙目标，输出 BaseColor/Normal 显式引用该 UV。
BaseColor 通过 EMIT 求原颜色表达式，避免金属材质的 DIFFUSE pass 变黑。Normal 同样读取
原图，两次烘焙完成后才替换输入。所有修改仅发生在 worker，不写回源 blend。

存在烘焙材质时不导出 Blender 自动生成的 tangent，因为它未必对应 normalTexture 的 UV；
glTF 消费端需从 normal UV 重建切线基，Truvis 已有缺失 tangent 的处理路径。其它通道保留
原节点图，是否可转录由 Blender glTF exporter 决定；此脚本不是任意材质图的完整烘焙器，
也不保证所有 Blender 效果均可由 glTF 或 Truvis 表达。

转换将当前帧的 mesh/curve/font 等求值结果冻结为静态几何；导出节点变换也固定为当前帧，
不导出动画、skin 和顶点色。shape key 和 armature modifier 的当前结果可烘入 mesh，但这不是动态动画
转换能力。camera/light 和 Blender 可转录的材质属性会保留；BLEND 会降级为 Opaque；
必需的 KHR_lights_punctual 仍会被当前 Truvis Reader 拒绝，转换成功不等于运行时支持。

`scene/validate_gltf.py` preflights glTF 2.0 external resources and core references:

```text
python scripts/scene/validate_gltf.py --scene <output-directory>/scene.gltf
```

预检拒绝错误的 JSON/容器结构、非整数或越界索引、缺失文件、过短 buffer 和超出声明
buffer 长度的 bufferView。覆盖基础纹理、材质、几何、scene/node、sparse、morph、skin
和 animation 的核心索引。buffer/image 必须使用包内相对文件 URI；百分号与 UTF-8 严格
解码一次，拒绝绝对路径、网络/data URI、query/fragment 及经 symlink 越出目录的路径。
编码后的文件名字符（如 `%23`）仍按文件名处理，不再解释为 URI 分隔符。

版本判断与 Rust Reader 一致：接受 major 2，minVersion 仅支持 2.0，必需扩展沿用 Reader
支持名单。预检读取 accessor 内容，拒绝非法布局、sparse 索引、非有限值、primitive 越界
索引和属性长度；BLEND、顶点色、morph、skin、动画与 transmission texture 按对象警告，
非透射 IOR 忽略。skin 表示未蒙皮基础网格 + 现有 node transform，不承诺 bind pose。

这仍是发布前预检：不解码图像，不完整校验 glTF schema、node 无环性、几何退化和材质数值域。
最终能否注册几何仍由 Rust Reader 决定；不把 preflight 成功当成 GPU 或画面验收。

The converter intentionally has no Bistro-specific scene or material names and
does not emit a manifest or `export.json`.

## Nushell helpers

The `nu/` scripts hold imperative environment setup previously embedded in
`justfile`. Public recipes remain in `justfile`; the helper scripts receive the
repository root and explicit arguments.
