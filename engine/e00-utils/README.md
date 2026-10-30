# e00-utils

`engine/e00-utils/` 保存不依赖 Engine/Renderer 运行时代码结构的 workspace 工具。工具只通过显式配置、
文件系统输入和窄公共 API 工作，不拥有 shader ABI、RenderGraph、GPU resource 或线程生命周期。

- `truvis-fetch-res/`：按 `resources.toml` 下载和准备外部资源。资源名称 `name` 必须唯一；缺省只处理
  `download_by_default = true` 的资源，也可以通过 `just fetch-res <name>...` 按名称选择，或使用
  `just fetch-res --all` 处理全部资源。
  支持 `file`、`zip`、`7z` 和 `git`；压缩包的 `rename_to` 是解压目标目录名，不剥离包内顶层目录。
  所有类型共享 Windows 名称校验，`name` 与 `rename_to` 必须是普通单层名称，不得使用 `.truvis-` 前缀。
  ZIP/7z 还预检包内路径；7z 使用纯 Rust 解压，不依赖系统 7-Zip。
  HTTP 缓存位于 `.temp/resource/http/<完整 URL 的 SHA-256>/`，流式下载完整后发布 `payload` 和 `sha256`；
  复用前校验内容摘要，损坏或缺少摘要时重新下载。摘要不认证上游版本；旧 basename 缓存不再读取，也不自动删除。
  file/ZIP/7z 在目标同级 `.truvis-<rename_to>.staging` 准备完整内容，旧目标暂存到 `.previous` 后才发布；
  发布失败恢复旧目标，下次运行恢复中断的替换。目标、staging 和 previous 不允许是链接/junction。
  `.temp/resource/published/` 保存来源/类型/目标和下载摘要；访问缓存前先核对记录与目标类型，匹配则直接跳过，目标内容被手工修改时需要
  `force_overwrite`。该选项用已校验缓存重建目标；`force_download` 重新下载并发布，失败保留旧缓存及旧目标。
  receipt 在目标成功发布后通过同目录临时文件 rename 发布。缓存与发布目录由下载器独占，不支持并发刷新；单资源可重试，不保证整批事务或断电持久性。
  Git 固定完整 commit 或本地已存在 tag 仅在 HEAD、干净 checkout 和所需 submodule 全部匹配时跳过；
  浮动分支先 fetch 再比较目标 commit，相同则跳过 checkout。复用 checkout 时原地 fetch/detach/update submodule，
  不重新 clone；只在全新获取或 force 时使用 staging。主仓库及已初始化 submodule 的本地修改、额外/ignored 文件和 stash
  都会阻止普通更新；缺失或版本不匹配的干净 submodule 会更新。原地更新失败返回错误，下次重新检查完整状态，
  不写虚假完成记录。force 允许替换，但 staging 准备失败仍保留旧目标。
  缺失配置或任一已选择资源失败时，CLI 返回失败退出码。
- `truvis-path/`：读取根目录 `paths.toml`，提供 workspace、shader、binding、temp 等可信物理目录；不理解
  shader package、entry 或 binding schema。
  `PathUtils` 的词法归一化不访问文件系统：保留相对路径中无法抵消的 `..`，绝对路径不越过根；不作为路径权限校验。
  `paths.toml` 不自动生成 Cargo/CMake/Nushell 配置；改动 `build`、`external`、`config` 等映射时，需同时核对
  `.cargo/config.toml`、CMake presets/SDK 路径和 `scripts/nu/` 的固定路径。当前这些入口的默认值一致。
- `truvis-shader-manifest/`：依赖 `truvis-path`，解析并校验 shader package、编译器、输出前缀与 binding
  配置；shared input 的 layer 必须与 include root 下的 `abi/` 或 `lib/` 前缀一致。
  不检查目录和编译器是否存在。
- `truvis-shader-build/`：由 manifest 驱动的 SPIR-V 全量/增量编译与依赖校验。
  源码预检只接受完整字面量 include/import，处理注释和续行后按 owner 逐边检查引用；被引用文本即使
  没有标准扩展也检查，并拒绝源文件相对目录遮蔽 include root。宏路径不能交给扁平 depfile 验证层级。
  entry/shared input 按 SHA-256 内容摘要判断变化；仅更新时间戳不失效。任务发现和输入遍历错误直接报错，
  在输出清理前完成全部输入检查。只解析当前任务使用的编译器；bare 名称从 PATH 查找（Windows 补 `.exe`），
  指纹与执行使用同一绝对路径。编译器 executable 内容 hash 或同目录 DLL 路径/长度/mtime 变化使本轮全部任务失效，未使用工具缺失不阻塞构建。
  DLL 不再全量读取；同长度同时间戳替换 DLL、目录外动态库或环境变化需要 `--force`，构建期间输入应保持稳定。
  保存新 manifest 前删除全部待编译任务的正式 depfile；每个任务执行前删除旧输出；编译器先写临时 depfile，退出成功、输出存在且依赖校验通过后
  才发布正式 depfile。失败或中止留下的临时 depfile 不能使后续增量构建跳过任务；启动失败按任务返回错误。
  整批输出先预检：拒绝目的地重复、文件/目录冲突、保留目录占用和输出根内链接/junction；历史归属仅允许
  非保留目录下的 SPIR-V。状态先记为未完成并保存旧/新输出并集，编译和清理全部成功后才发布当前成功集合。
  版本匹配的未完成 manifest 仍用于比较输入；正式 .d、输出存在与依赖有效性共同决定单任务是否可复用。失败后删除或更名 entry 仍能清理本轮已产生的文件。
  状态通过同目录临时文件加 rename 发布；发布失败保留上一次完整 JSON，不保证断电持久性。
  构建失败后不能使用本轮输出集合；不保证多个任务的事务回滚，同一输出目录不支持并发运行多个构建器。
- `truvis-shader-binding-codegen/`：消费 manifest 已解析的 header、include roots 与输出路径，统一执行
  bindgen、类型重命名、content hash 和 write-if-changed。
shader 源码与 ABI owner 仍分别位于 `engine/shader/`、`renderer/shader/`；工具层不定义 namespace、
allowlist 或跨 crate re-export policy。CXX project、工具和 binding 统一位于 workspace 根 `cxx/`。
