//! Shader 编译工具
//!
//! 将指定目录下的 shader 入口文件编译为 SPIR-V 文件，输出到 `build/shader` 目录。
//!
//! 本 binary 只负责“发现入口文件、判断是否需要重新编译、调用具体 shader compiler、
//! 维护增量 manifest”。具体的 glslc/dxc/slangc 参数归属在各 backend 模块中，shader 侧
//! 结构体绑定仍由 `truvis-shader-binding` 负责，避免把编译产物生成和 Rust ABI 绑定生成混在一起。

mod common;
mod glsl;
mod hlsl;
mod slang;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use common::{EnvPath, ShaderCompileTask, ShaderCompiler, ShaderCompilerType};
use glsl::GlslCompiler;
use hlsl::HlslCompiler;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use slang::SlangCompiler;
use truvis_logs::{LogFilePath, TruvisLogger};
use truvis_path::TruvisPath;

// manifest schema 版本用于处理 JSON 字段结构变化；一旦结构变化到无法安全兼容旧文件，
// 提升该版本即可让旧 manifest 自动失效，并触发下一轮完整输入判断。
const MANIFEST_VERSION: u32 = 2;
// 编译器参数属于 shader ABI 的隐性输入：即使 shader 源文件没有变化，debug info、matrix layout、
// target-env 或 entrypoint 规则变化也会改变 SPIR-V 语义。用单独版本隔离这类“命令行契约”变更。
const COMPILER_ARGS_VERSION: &str = "shader-compiler-args-v2";

/// 命令行只暴露最小控制面。
///
/// `shader-build` 通常由 `just shader` 调用，默认依赖 manifest 做增量判断；`--force`
/// 是给调试 shader 编译器参数、清理坏缓存或验证全量编译链路时使用的逃生口。
struct CliOptions {
    force: bool,
}

impl CliOptions {
    fn parse() -> Result<Self, String> {
        let mut force = false;
        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--force" | "-f" => force = true,
                "--help" | "-h" => return Err("Usage: shader-build [--force]".to_string()),
                _ => return Err(format!("Unsupported shader-build arg '{arg}'")),
            }
        }

        Ok(Self { force })
    }
}

/// `shader-build` 的路径上下文。
///
/// 路径转换集中在这里有两个目的：
/// - manifest 中始终记录 workspace 相对路径，并统一使用 `/`，这样 JSON 不依赖 Windows 路径分隔符；
/// - 需要访问真实文件系统时，再从 manifest 路径还原为当前 workspace 下的本地路径。
///
/// 这个类型只表达路径所有权与路径格式契约，不承担文件扫描、编译或 manifest 语义判断。
struct ShaderBuildLayout {
    workspace_dir: PathBuf,
    manifest_path: PathBuf,
}

impl ShaderBuildLayout {
    fn new() -> Self {
        let workspace_dir = TruvisPath::workspace_path();
        let manifest_path = EnvPath::shader_build_path().join(".state").join("shader-build.json");
        Self {
            workspace_dir,
            manifest_path,
        }
    }

    fn manifest_path(&self) -> &Path {
        &self.manifest_path
    }

    fn workspace_path_from_manifest(&self, relative_path: &str) -> PathBuf {
        self.workspace_dir.join(relative_path.replace('/', "\\"))
    }

    /// 将文件路径转换为稳定的 manifest key。
    ///
    /// 这里故意不做 canonicalize：canonicalize 可能触发磁盘访问、解析 junction/symlink，
    /// 也会把不存在的旧输出路径变成错误。manifest 只需要在当前 workspace 内稳定比较，
    /// 所以使用词法上的 strip_prefix 更符合这个轻量 build helper 的职责。
    fn relative_slash_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.workspace_dir).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }
}

/// shader 编译流程的协调者。
///
/// Runner 负责把“输入快照、增量判断、输出清理、并行编译、manifest 保存”按固定顺序串起来。
/// 它不持有 compiler 进程或跨轮状态；每次运行都从文件系统和 manifest 重新构建决策，避免引入
/// 常驻缓存导致的隐藏生命周期问题。
struct ShaderBuildRunner {
    layout: ShaderBuildLayout,
    force: bool,
}

impl ShaderBuildRunner {
    fn new(force: bool) -> Self {
        Self {
            layout: ShaderBuildLayout::new(),
            force,
        }
    }

    fn run(&self) -> Result<(), String> {
        // 这三条日志是 shader 编译问题排查的第一入口：include、entry 和 output 任一目录解析错误，
        // 都会直接表现为找不到输入或运行时加载不到 SPIR-V。
        log::info!("Shader api path: {:?}", EnvPath::shader_api_path());
        log::info!("Shader entry path: {:?}", EnvPath::shader_entry_path());
        log::info!("Shader output path: {:?}", EnvPath::shader_build_path());

        let previous_manifest = ShaderBuildManifest::load(self.layout.manifest_path())?;
        let tasks = self.collect_tasks();
        let shared_inputs = self.collect_shared_inputs()?;
        // entry shader 自身可以按文件粒度判断；api/lib 和 entry 下的 include 文件则作为
        // 全局 shared inputs 处理。当前工具不解析 Slang/GLSL include graph，因此 shared
        // inputs 一旦变化，就保守重编所有入口，避免少编某个间接依赖它的 shader。
        let shared_inputs_changed =
            previous_manifest.as_ref().is_none_or(|manifest| manifest.shared_inputs != shared_inputs);

        self.remove_stale_outputs(previous_manifest.as_ref(), &tasks)?;

        // task_map 以入口 shader 路径为 key，而不是以输出路径为 key。入口文件才是编译任务的身份；
        // 输出路径只是当前命名规则推导出的产物位置，未来如果输出扩展名规则调整，应让任务本身失效。
        let previous_tasks = previous_manifest.as_ref().map(ShaderBuildManifest::task_map).unwrap_or_default();

        let mut next_task_manifests = Vec::new();
        let mut tasks_to_compile = Vec::new();
        for task in tasks {
            let task_manifest = ShaderTaskManifest::from_task(&self.layout, &task)?;
            let previous_task = previous_tasks.get(&task_manifest.shader_path);
            let output_path = self.layout.workspace_path_from_manifest(&task_manifest.output_path);
            // 单个入口的增量判断只看该入口文件、编译阶段、编译器类型、输出是否存在，
            // 以及 shared inputs 是否整体变化。这样修改一个独立 entry 时能只重编它；
            // 修改 shared/include 时则走保守全量入口重编，优先保证 ABI 与 SPIR-V 一致。
            let needs_compile = self.force
                || shared_inputs_changed
                || previous_task.is_none_or(|old_task| old_task != &task_manifest)
                || !output_path.is_file();

            if needs_compile {
                tasks_to_compile.push(task.clone());
            }

            // 即使本轮不需要编译，也要写入下一份 manifest。这样 manifest 始终描述“当前文件树”，
            // 不会因为一次跳过编译而保留已经被删除或改名的旧任务。
            next_task_manifests.push(task_manifest);
        }

        self.compile_tasks(&tasks_to_compile)?;

        ShaderBuildManifest {
            version: MANIFEST_VERSION,
            shared_inputs,
            tasks: next_task_manifests,
        }
        .save(self.layout.manifest_path())?;

        log::info!(
            "Shader compilation completed. compiled={}, skipped={}",
            tasks_to_compile.len(),
            previous_tasks.len().saturating_sub(tasks_to_compile.len())
        );
        Ok(())
    }

    /// 只收集可以直接交给具体 compiler 的入口 shader。
    ///
    /// `ShaderCompileTask::new` 承担“文件名到 stage/compiler/output”的项目约定判断；这里保持扫描逻辑
    /// 纯粹，避免在 runner 中复制后缀解析规则。
    fn collect_tasks(&self) -> Vec<ShaderCompileTask> {
        let mut tasks = walkdir::WalkDir::new(EnvPath::shader_entry_path())
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_file())
            .filter_map(|entry| ShaderCompileTask::new(&entry))
            .collect::<Vec<_>>();
        tasks.sort_by(|left, right| left.shader_path.cmp(&right.shader_path));
        tasks
    }

    /// 收集会影响多个入口的共享输入。
    ///
    /// 当前工具没有解析 include graph，因此共享输入采用保守模型：`api/`、`lib/` 和 entry 下不能直接
    /// 编译的 include-like 文件任一变化，都让所有入口重新编译。这样牺牲少量增量精度，换取 shader ABI
    /// 和 SPIR-V 产物不会因为漏掉间接依赖而失配。
    fn collect_shared_inputs(&self) -> Result<Vec<FileStamp>, String> {
        let mut inputs = Vec::new();

        for root in [
            EnvPath::shader_api_path().to_path_buf(),
            EnvPath::shader_root_path().join("lib"),
            EnvPath::shader_entry_path().to_path_buf(),
        ] {
            if !root.exists() {
                continue;
            }

            for entry in walkdir::WalkDir::new(&root).into_iter().filter_map(Result::ok) {
                if !entry.path().is_file() {
                    continue;
                }

                // entry 下无法直接编译的 .slangi / .inc.glsl / works/*.glsl 等文件通常作为 include
                // 被某个入口引用。解析完整 include graph 需要理解 Slang/GLSL 各自的 include
                // 语义和编译器搜索路径，复杂度不值得放到这个轻量 build helper 中；因此这类
                // 文件变化时统一触发保守重编，用少量额外编译时间换取不会漏编。
                if entry.path().starts_with(EnvPath::shader_entry_path()) && ShaderCompileTask::new(&entry).is_some() {
                    continue;
                }

                inputs.push(FileStamp::from_path(&self.layout, entry.path())?);
            }
        }

        inputs.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(inputs)
    }

    /// 并行编译本轮失效的入口 shader。
    ///
    /// 每个任务独立创建输出目录并调用 backend compiler；错误统一收集后再返回，便于一次运行看到
    /// 多个 shader 的失败信息。这里不让单个任务 panic 终止整个 rayon worker 池。
    fn compile_tasks(&self, tasks: &[ShaderCompileTask]) -> Result<(), String> {
        if tasks.is_empty() {
            log::info!("Shader inputs unchanged; skip shader compiler invocations.");
            return Ok(());
        }

        let errors = tasks
            .par_iter()
            .filter_map(|task| {
                log::info!("Compiling shader: {:?}", task.shader_path);

                self.compile_task(task).err()
            })
            .collect::<Vec<_>>();

        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    }

    /// 编译单个入口 shader。
    ///
    /// 输出目录创建放在 runner 中，而不是各 backend 中，是因为目录生命周期与构建产物布局有关；
    /// backend 只应该关心如何把一个已定义好的任务交给 glslc/dxc/slangc。
    fn compile_task(&self, task: &ShaderCompileTask) -> Result<(), String> {
        if let Some(parent) = task.output_path.parent()
            && let Err(err) = std::fs::create_dir_all(parent)
        {
            return Err(format!("无法创建 shader 输出目录 {}: {err}", parent.display()));
        }

        let compiler = Self::compiler_for(task.compiler_type);
        compiler.compile(task)
    }

    /// 删除上一轮 manifest 管理过、但当前任务集中已经不存在的输出文件。
    ///
    /// 删除边界必须以旧 manifest 为准：只有曾被本工具声明管理过的产物才会被清理，避免把
    /// `build/shader` 中用户临时放置的调试文件或其它工具产物误删。
    fn remove_stale_outputs(
        &self,
        previous_manifest: Option<&ShaderBuildManifest>,
        current_tasks: &[ShaderCompileTask],
    ) -> Result<(), String> {
        let Some(previous_manifest) = previous_manifest else {
            return Ok(());
        };

        let current_outputs = current_tasks
            .iter()
            .map(|task| self.layout.relative_slash_path(&task.output_path))
            .collect::<BTreeSet<_>>();

        for old_task in &previous_manifest.tasks {
            if current_outputs.contains(&old_task.output_path) {
                continue;
            }

            let output_path = self.layout.workspace_path_from_manifest(&old_task.output_path);
            if output_path.exists() {
                std::fs::remove_file(&output_path)
                    .map_err(|err| format!("无法删除旧 shader 输出 {}: {err}", output_path.display()))?;
                log::info!("Removed stale shader output: {}", output_path.display());
            }
        }

        Ok(())
    }

    /// 编译器 backend 的归属保持在 runner 内部，避免让 common.rs 反向依赖具体 backend 模块。
    fn compiler_for(compiler_type: ShaderCompilerType) -> Box<dyn ShaderCompiler> {
        match compiler_type {
            ShaderCompilerType::Glsl => Box::new(GlslCompiler::new()),
            ShaderCompilerType::Hlsl => Box::new(HlslCompiler::new()),
            ShaderCompilerType::Slang => Box::new(SlangCompiler::new()),
        }
    }
}

/// 一次 `shader-build` 运行持久化到磁盘的增量状态。
///
/// manifest 不试图成为完整依赖图，只保存“共享输入快照 + 每个入口任务快照”。这和当前工具的职责匹配：
/// 入口文件可以细粒度增量，shared/include 变更则保守全量重编。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ShaderBuildManifest {
    version: u32,
    shared_inputs: Vec<FileStamp>,
    tasks: Vec<ShaderTaskManifest>,
}

impl ShaderBuildManifest {
    /// 读取上一轮 manifest。
    ///
    /// 版本不匹配时返回 `None` 而不是报错，表示旧状态不再可信但不阻止构建继续；JSON 损坏或读文件失败
    /// 则返回错误，因为这通常代表磁盘状态或工具写入流程异常，需要显式暴露。
    fn load(path: &Path) -> Result<Option<Self>, String> {
        if !path.is_file() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(path)
            .map_err(|err| format!("无法读取 shader manifest {}: {err}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content)
            .map_err(|err| format!("无法解析 shader manifest {}: {err}", path.display()))?;
        if manifest.version == MANIFEST_VERSION { Ok(Some(manifest)) } else { Ok(None) }
    }

    /// 保存当前 manifest。
    ///
    /// 写入前比较内容，避免 no-op 构建反复刷新 `.state/shader-build.json` 的修改时间，从而干扰
    /// 开发者判断“这次构建是否真的改变了状态”。
    fn save(&self, path: &Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|err| format!("无法序列化 shader manifest {}: {err}", path.display()))?;
        Self::write_if_changed(path, content.as_bytes())
    }

    fn task_map(&self) -> BTreeMap<String, ShaderTaskManifest> {
        self.tasks.iter().map(|task| (task.shader_path.clone(), task.clone())).collect::<BTreeMap<_, _>>()
    }

    /// 原子性不是这里的目标，少写才是目标。
    ///
    /// shader manifest 是可重建缓存；如果写入失败，下一轮可以通过 `--force` 或删除 `.state` 恢复。
    /// 因此这里优先保持实现简单，只保证写入前创建父目录并在内容不变时跳过写盘。
    fn write_if_changed(path: &Path, content: &[u8]) -> Result<(), String> {
        if path.is_file() {
            let old_content =
                std::fs::read(path).map_err(|err| format!("无法读取已有文件 {}: {err}", path.display()))?;
            if old_content == content {
                return Ok(());
            }
        }

        let parent = path.parent().ok_or_else(|| format!("无法获取目标目录: {}", path.display()))?;
        std::fs::create_dir_all(parent).map_err(|err| format!("无法创建目录 {}: {err}", parent.display()))?;
        std::fs::write(path, content).map_err(|err| format!("无法写入文件 {}: {err}", path.display()))
    }
}

/// 单个入口 shader 的增量快照。
///
/// 每个入口的 manifest 不只记录源文件时间戳，也记录 stage、compiler 类型和参数版本。这些字段是
/// shader ABI 的隐性输入：未来如果调整 slangc/dxc/glslc 参数，只需提升 `COMPILER_ARGS_VERSION`，
/// 就能让旧 manifest 失效，避免继续复用旧 SPIR-V。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct ShaderTaskManifest {
    shader_path: String,
    output_path: String,
    shader_input: FileStamp,
    shader_stage: String,
    compiler_type: String,
    compiler_args_version: String,
}

impl ShaderTaskManifest {
    /// 从编译任务生成可持久化快照。
    ///
    /// 这里保存的是用于判断“任务语义是否变化”的最小集合；不保存完整命令行，避免 manifest
    /// 和 backend 参数实现重复。参数变化统一通过 `COMPILER_ARGS_VERSION` 表达。
    fn from_task(layout: &ShaderBuildLayout, task: &ShaderCompileTask) -> Result<Self, String> {
        Ok(Self {
            shader_path: layout.relative_slash_path(&task.shader_path),
            output_path: layout.relative_slash_path(&task.output_path),
            shader_input: FileStamp::from_path(layout, &task.shader_path)?,
            shader_stage: format!("{:?}", task.shader_stage),
            compiler_type: format!("{:?}", task.compiler_type),
            compiler_args_version: COMPILER_ARGS_VERSION.to_string(),
        })
    }
}

/// 文件内容变化的轻量判定信息。
///
/// 这里使用路径、长度和修改时间，而不是读取文件 hash：shader build 是开发期工具，输入数量和文件体积
/// 都不大，但每轮读取全部内容仍然没有必要。mtime 精度不足或外部工具异常保留时间戳时，可用 `--force`
/// 绕过该轻量判定。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
struct FileStamp {
    path: String,
    len: u64,
    modified_ms: u64,
}

impl FileStamp {
    /// 为 manifest 捕获一个文件快照。
    ///
    /// 路径通过 `ShaderBuildLayout` 统一转成 workspace 相对 `/` 形式，保证排序和 JSON diff 在 Windows
    /// 环境下也稳定可读。
    fn from_path(layout: &ShaderBuildLayout, path: &Path) -> Result<Self, String> {
        let metadata =
            std::fs::metadata(path).map_err(|err| format!("无法读取文件元数据 {}: {err}", path.display()))?;
        Ok(Self {
            path: layout.relative_slash_path(path),
            len: metadata.len(),
            modified_ms: Self::modified_ms(&metadata)?,
        })
    }

    /// 将平台文件时间转换为 manifest 可序列化的毫秒值。
    ///
    /// 如果文件系统返回早于 UNIX_EPOCH 的时间，使用默认值兜底；这是缓存失效判断，不参与运行时渲染
    /// 语义，因此宁可让该文件看起来“很旧”，也不因为异常时间戳中断整个构建。
    fn modified_ms(metadata: &std::fs::Metadata) -> Result<u64, String> {
        let modified = metadata.modified().map_err(|err| format!("无法读取文件修改时间: {err}"))?;
        let millis = modified.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        Ok(millis.min(u128::from(u64::MAX)) as u64)
    }
}

fn main() -> Result<(), String> {
    // 日志文件放在 temp 目录，避免 shader build 作为开发工具运行时污染源码树或 build/shader 产物目录。
    TruvisLogger::init_with_file(LogFilePath::current_exe(TruvisPath::temp_dir()));

    let options = CliOptions::parse()?;
    ShaderBuildRunner::new(options.force).run()
}
