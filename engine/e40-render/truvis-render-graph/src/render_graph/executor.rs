//! RenderGraph 构建器和执行器
//!
//! 提供 `RenderGraphBuilder` 用于构建渲染图，
//! `CompiledGraph` 用于缓存编译结果并执行渲染。

use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
};

use ash::vk;
use itertools::Itertools;
use slotmap::SecondaryMap;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::commands::submit_info::GfxSubmitInfo;
use truvis_render_foundation::handles::{GfxImageHandle, GfxImageViewHandle};
use truvis_render_foundation::resource_access::GfxResourceAccess;

use crate::render_graph::barrier::{PassBarriers, RgImageBarrierDesc};
use crate::render_graph::export_info::RgExportInfo;
use crate::render_graph::image_resource::RgImageResource;
use crate::render_graph::pass::{RgLambdaPassWrapper, RgPass, RgPassBuilder, RgPassContext, RgPassNode, RgPassWrapper};
use crate::render_graph::resource_handle::RgImageHandle;
use crate::render_graph::resource_manager::RgResourceManager;
use crate::render_graph::resource_state::RgImageState;
use crate::render_graph::semaphore_info::RgSemaphoreInfo;

/// RenderGraph 构建器
///
/// 用于声明式构建渲染图。
///
/// # 使用流程
///
/// 1. 创建 builder: `RenderGraphBuilder::new()`
/// 2. 导入外部资源: `builder.import_image(...)`
/// 3. 添加 Pass: `builder.add_pass("name", pass)`
/// 4. 编译: `builder.compile()`
/// 5. 执行: `compiled.execute(...)`
///
/// # 生命周期
///
/// `'a` 是 Pass 可以借用的外部资源的生命周期。
/// 这允许 Pass 直接引用外部的 pipeline、geometry 等资源，
pub struct RenderGraphBuilder<'a> {
    /// 资源注册表
    resources: RgResourceManager,

    /// Pass 节点列表（按添加顺序）
    passes: Vec<RgPassNode<'a>>,

    /// 导出资源信息：指定资源的最终状态和可选的 signal semaphore
    export_images: HashMap<RgImageHandle, RgExportInfo>,

    signal_semaphores: Vec<RgSemaphoreInfo>,
}

impl Default for RenderGraphBuilder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

// 创建与初始化
impl<'a> RenderGraphBuilder<'a> {
    /// 创建新的 RenderGraph 构建器
    pub fn new() -> Self {
        Self {
            resources: RgResourceManager::new(),
            passes: Vec::new(),
            export_images: HashMap::new(),
            signal_semaphores: Vec::new(),
        }
    }
}

// build 阶段
impl<'a> RenderGraphBuilder<'a> {
    /// 导入外部图像资源
    ///
    /// # 参数
    /// - `name`: 资源调试名称
    /// - `image_handle`: 物理图像句柄（来自 GfxResourceRegistry）
    /// - `view_handle`: 可选的图像视图句柄
    /// - `format`: 图像格式（用于推断 barrier aspect）
    /// - `initial_state`: 图像的初始状态
    /// - `wait_semaphore`: 可选的外部 semaphore 等待（在首次使用此资源前等待）
    ///
    /// # 返回
    /// RenderGraph 内部的图像句柄
    pub fn import_image(
        &mut self,
        name: impl Into<String>,
        image_handle: GfxImageHandle,
        view_handle: Option<GfxImageViewHandle>,
        format: vk::Format,
        initial_state: RgImageState,
        wait_semaphore: Option<RgSemaphoreInfo>,
    ) -> RgImageHandle {
        self.resources.register_image(RgImageResource::imported(
            name,
            image_handle,
            view_handle,
            format,
            initial_state,
            wait_semaphore,
        ))
    }

    /// 导出图像资源
    ///
    /// 声明资源在渲染图执行完成后的最终状态，并可选地发出 semaphore 信号。
    /// 这会在图的末尾插入必要的 barrier 将资源转换到指定的 final_state。
    ///
    /// # 参数
    /// - `handle`: 要导出的图像句柄
    /// - `final_state`: 资源的最终状态（layout, access, stage）
    /// - `signal_semaphore`: 可选的 semaphore 信号
    ///
    /// # 返回
    /// 返回 `&mut Self` 以支持链式调用
    pub fn export_image(
        &mut self,
        handle: RgImageHandle,
        final_state: RgImageState,
        signal_semaphore: Option<RgSemaphoreInfo>,
    ) -> &mut Self {
        assert!(self.resources.get_image(handle).is_some(), "cannot export unregistered graph image {:?}", handle);
        self.export_images.insert(
            handle,
            RgExportInfo {
                final_state,
                signal_semaphore,
            },
        );
        self
    }

    pub fn signal_semaphore(&mut self, semaphore: RgSemaphoreInfo) -> &mut Self {
        self.signal_semaphores.push(semaphore);
        self
    }

    /// 添加 Pass
    ///
    /// # 参数
    /// - `name`: Pass 名称（用于调试和性能分析）
    /// - `pass`: 实现了 `RgPass` trait 的 Pass 对象
    ///
    /// # 返回
    /// 返回 `&mut Self` 以支持链式调用
    pub fn add_pass<P: RgPass + 'a>(&mut self, name: impl Into<String>, mut pass: P) -> &mut Self {
        let name = name.into();
        // 名称会作为 Vulkan debug label；在 setup 和任何 command 录制前拒绝确定无效的输入。
        assert!(!name.as_bytes().contains(&0), "graph pass name must not contain NUL");

        // 创建 PassBuilder 供 Pass 声明依赖
        let mut builder = RgPassBuilder {
            name: name.clone(),
            image_reads: Vec::new(),
            image_writes: Vec::new(),
        };

        // 调用 Pass 的 setup 方法
        pass.setup(&mut builder);

        // setup 的全部声明校验通过后才发布节点，避免 compile 静默跳过不存在的资源。
        for (handle, _) in builder.image_reads.iter().chain(&builder.image_writes) {
            assert!(
                self.resources.get_image(*handle).is_some(),
                "pass '{}' references unregistered graph image {:?}",
                name,
                handle
            );
        }

        // 创建 PassNode
        let node = RgPassNode {
            name,
            image_reads: builder.image_reads,
            image_writes: builder.image_writes,
            executor: Box::new(RgPassWrapper { pass }),
        };

        self.passes.push(node);
        self
    }

    pub fn add_pass_lambda<S, E>(&mut self, name: impl Into<String>, setup_fn: S, execute_fn: E) -> &mut Self
    where
        S: FnMut(&mut RgPassBuilder) + 'a,
        E: Fn(&RgPassContext<'_>) + 'a,
    {
        let pass = RgLambdaPassWrapper::new(setup_fn, execute_fn);
        self.add_pass(name, pass)
    }
}

// compile 阶段
impl<'a> RenderGraphBuilder<'a> {
    /// 编译渲染图
    ///
    /// 固定 pass 插入顺序并计算每个 pass 前需要的 barrier。
    ///
    /// # 返回
    /// 编译后的 `CompiledGraph`；再次执行前必须重新满足导入状态与 semaphore 前置。
    ///
    pub fn compile(self) -> CompiledGraph<'a> {
        let _span = tracy_client::span!("RenderGraphBuilder::compile");

        let pass_count = self.passes.len();
        let execution_order = (0..pass_count).collect_vec();

        // 计算每个 Pass 的 barriers（同时返回最终的资源状态用于计算 epilogue barriers）
        let (barriers, final_image_states) = self.compute_barriers(&execution_order);

        let (wait_semaphores, signal_semaphores) = self.collect_semaphores();

        // 计算 epilogue barriers：将导出资源从最后使用状态转换到 final_state
        let epilogue_barriers = self.compute_epilogue_barriers(&final_image_states);

        CompiledGraph {
            resources: self.resources,
            passes: self.passes,
            execution_order,
            barriers,
            epilogue_barriers,
            wait_semaphores,
            signal_semaphores,
        }
    }

    /// 图只生成一次 batch；共享完成事件的多个资源合并为一次 wait/signal。
    fn collect_semaphores(&self) -> (Vec<RgSemaphoreInfo>, Vec<RgSemaphoreInfo>) {
        let mut waits: Vec<RgSemaphoreInfo> = Vec::new();
        for wait in self.resources.iter_images().filter_map(|(_, res)| res.wait_semaphore()) {
            assert_ne!(wait.semaphore, vk::Semaphore::null(), "graph wait semaphore must be non-null");
            if let Some(existing) = waits.iter_mut().find(|existing| existing.semaphore == wait.semaphore) {
                existing.merge_wait(wait);
            } else {
                waits.push(wait);
            }
        }

        let mut signals: Vec<RgSemaphoreInfo> = Vec::new();
        for signal in self
            .export_images
            .values()
            .filter_map(|info| info.signal_semaphore)
            .chain(self.signal_semaphores.iter().copied())
        {
            assert_ne!(signal.semaphore, vk::Semaphore::null(), "graph signal semaphore must be non-null");
            assert_ne!(signal.value, Some(0), "timeline signal must increase the nonnegative counter");
            if let Some(existing) = signals.iter_mut().find(|existing| existing.semaphore == signal.semaphore) {
                existing.merge_signal(signal);
            } else {
                signals.push(signal);
            }
        }

        for wait in &waits {
            if let Some(signal) = signals.iter().find(|signal| signal.semaphore == wait.semaphore) {
                assert_eq!(wait.value.is_some(), signal.value.is_some(), "conflicting wait/signal semaphore types");
                if let (Some(wait_value), Some(signal_value)) = (wait.value, signal.value) {
                    assert!(signal_value > wait_value, "timeline signal must be greater than this batch's wait");
                }
            }
        }
        (waits, signals)
    }

    /// 计算 epilogue barriers
    ///
    /// 将导出资源从最后使用状态转换到声明的 final_state
    fn compute_epilogue_barriers(
        &self,
        final_image_states: &SecondaryMap<RgImageHandle, RgImageState>,
    ) -> PassBarriers {
        let mut epilogue = PassBarriers::new();

        for (&handle, export_info) in &self.export_images {
            let current_state = *final_image_states.get(handle).expect("exported image must have a tracked state");
            let res = self.resources.get_image(handle).expect("exported image must be registered");
            // 相同 layout/access 的写后依赖仍需屏障，统一交给 needs_barrier 判定。
            epilogue.add_image_barrier(
                RgImageBarrierDesc::new(handle, current_state, export_info.final_state).with_aspect(res.infer_aspect()),
            );
        }

        epilogue
    }

    /// 计算每个 Pass 需要的 barriers
    ///
    /// 模拟 pass 的执行顺序，跟踪资源的状态变化，生成必要的 barriers
    ///
    /// # 返回
    /// - barriers: 每个 Pass 的 barriers
    /// - final_image_states: 所有图像资源的最终状态（用于计算 epilogue barriers）
    fn compute_barriers(
        &self,
        execution_order: &[usize],
    ) -> (Vec<PassBarriers>, SecondaryMap<RgImageHandle, RgImageState>) {
        let mut barriers = vec![PassBarriers::new(); self.passes.len()];

        // 跟踪每个资源的当前状态 (使用 SecondaryMap)
        let mut image_states: SecondaryMap<RgImageHandle, RgImageState> = SecondaryMap::new();

        // 初始化状态
        for (handle, res) in self.resources.iter_images() {
            image_states.insert(handle, res.current_state);
        }

        let get_image_aspect = |handle: RgImageHandle| {
            let image_resource = self.resources.get_image(handle).unwrap();
            image_resource.infer_aspect()
        };

        for &pass_idx in execution_order {
            let pass = &self.passes[pass_idx];
            let pass_barriers = &mut barriers[pass_idx];

            // 同一 pass 的依赖覆盖全部声明；graph 不推断 pass 内部的 layout 转换。
            let mut image_usage: HashMap<RgImageHandle, RgImageState> = HashMap::new();
            for (handle, state) in pass.image_reads.iter().chain(&pass.image_writes) {
                let usage = image_usage.entry(*handle).or_insert(*state);
                assert_eq!(
                    usage.layout, state.layout,
                    "pass '{}' declares incompatible layouts for graph image {:?}",
                    pass.name, handle
                );
                *usage = Self::merge_states(*usage, *state);
            }

            // 为每个使用的图像生成 barrier
            for (handle, required_state) in image_usage {
                if let Some(&crt_state) = image_states.get(handle) {
                    let aspect = get_image_aspect(handle);

                    pass_barriers.add_image_barrier(
                        RgImageBarrierDesc::new(handle, crt_state, required_state).with_aspect(aspect),
                    );

                    let next_state = if crt_state.layout == required_state.layout
                        && crt_state.is_read_only()
                        && required_state.is_read_only()
                    {
                        Self::merge_states(crt_state, required_state)
                    } else {
                        required_state
                    };
                    image_states.insert(handle, next_state);
                }
            }
        }

        (barriers, image_states)
    }

    /// 合并同 layout 的访问范围；用于同 pass 声明，以及跨 pass 连续只读访问。
    fn merge_states(current: RgImageState, required: RgImageState) -> RgImageState {
        RgImageState::new(current.stage | required.stage, current.access | required.access, current.layout)
    }
}

/// 编译后的渲染图
///
/// 包含固定的执行顺序、barriers 与 semaphore 声明，不回写长期 image 状态。
/// 重复录制必须由调用方恢复导入前置；尤其不能重复使用已消费的 binary 信号或旧 timeline signal 值。
///
/// # 生命周期
///
/// `'a` 是 Pass 借用的外部资源的生命周期。
/// CompiledGraph 的生命周期不能超过这些外部资源。
pub struct CompiledGraph<'a> {
    /// 资源注册表
    resources: RgResourceManager,
    /// Pass 节点列表
    passes: Vec<RgPassNode<'a>>,
    /// 执行顺序（pass 添加顺序）
    execution_order: Vec<usize>,
    /// 每个 Pass 的 barriers（按 pass 索引）
    barriers: Vec<PassBarriers>,
    /// 尾声 barriers：将导出资源转换到最终状态
    epilogue_barriers: PassBarriers,
    /// 收集的外部 wait semaphores（来自导入资源）
    wait_semaphores: Vec<RgSemaphoreInfo>,
    /// 收集的外部 signal semaphores（来自导出资源）
    signal_semaphores: Vec<RgSemaphoreInfo>,
}

impl CompiledGraph<'_> {
    /// 获取执行顺序
    pub fn execution_order(&self) -> &[usize] {
        &self.execution_order
    }

    /// 获取 Pass 数量
    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }

    /// 获取 Pass 名称
    pub fn pass_name(&self, index: usize) -> &str {
        &self.passes[index].name
    }

    /// 执行渲染图
    ///
    /// pass 或录制过程 panic 时可能已有部分命令写入；调用方必须丢弃该录制结果，不能提交。
    /// graph 不重置 command，也不捕获异常后继续执行。再次录制须重新满足所有 import 前置。
    ///
    /// # 参数
    /// - `cmd`: 命令缓冲区（已经 begin）
    /// - `resource_manager`: 资源只读查询契约（用于获取物理资源）
    pub fn execute(&self, cmd: &GfxCommandBuffer, resource_manager: &dyn GfxResourceAccess) {
        let _span = tracy_client::span!("CompiledGraph::execute");

        // 在任何 graph 命令录制前核验全部导入，避免前面的 pass 已执行才发现后面的资源失效。
        let image_handles = self.resolve_images(resource_manager);

        // 按顺序执行 Pass
        for &pass_idx in &self.execution_order {
            let pass = &self.passes[pass_idx];
            let pass_barriers = &self.barriers[pass_idx];

            // 插入 barriers
            if pass_barriers.has_barriers() {
                self.record_barriers(cmd, pass_barriers, resource_manager);
            }

            // 开始 Pass debug label
            cmd.begin_label(&pass.name, truvis_gfx::basic::color::LabelColor::COLOR_PASS);

            // 执行 Pass
            let ctx = RgPassContext {
                cmd,
                resource_manager,
                image_handles: &image_handles,
            };
            pass.executor.execute(&ctx);

            // 结束 Pass debug label
            cmd.end_label();
        }

        // 录制 epilogue barriers（将导出资源转换到最终状态）
        if self.epilogue_barriers.has_barriers() {
            cmd.begin_label("rg-epilogue", truvis_gfx::basic::color::LabelColor::COLOR_PASS);
            self.record_barriers(cmd, &self.epilogue_barriers, resource_manager);
            cmd.end_label();
        }
    }

    /// 只读核验 native 身份；查询方必须在整个 execute 期间保持资源及映射稳定。
    fn resolve_images(
        &self,
        resource_manager: &dyn GfxResourceAccess,
    ) -> SecondaryMap<RgImageHandle, (GfxImageHandle, Option<GfxImageViewHandle>)> {
        let mut image_handles = SecondaryMap::new();
        let mut native_images = HashSet::new();
        for (handle, resource) in self.resources.iter_images() {
            let physical = resource.physical_handle().expect("graph image must be imported");
            let image = resource_manager.get_image(physical).expect("imported graph image is missing");
            assert!(image.handle() != vk::Image::null(), "imported graph image '{}' has been destroyed", resource.name);
            assert_eq!(resource.format, image.format(), "imported image '{}' format mismatch", resource.name);
            assert!(
                native_images.insert(image.handle()),
                "native image '{}' imported through multiple handles",
                resource.name
            );
            let view_handle = resource.physical_view_handle();
            if let Some(view_handle) = view_handle {
                let view = resource_manager.get_image_view(view_handle).expect("imported graph image view is missing");
                assert!(
                    view.handle() != vk::ImageView::null(),
                    "imported image view '{}' has been destroyed",
                    resource.name
                );
                assert_eq!(
                    view.image_handle(),
                    image.handle(),
                    "imported view '{}' belongs to another image",
                    resource.name
                );
            }
            image_handles.insert(handle, (physical, view_handle));
        }
        image_handles
    }

    /// 构建包含外部同步信息的 SubmitInfo
    ///
    /// 返回的 `GfxSubmitInfo` 包含了从导入资源收集的 wait semaphores
    /// 和从导出资源收集的 signal semaphores。
    /// 此方法只投影声明，不证明 commands 已完整录制并 end，也不延长任何 native owner 的生命周期。
    /// 调用方必须提交对应 graph 的完整命令，并保持 image/view/semaphore 存活到 GPU 使用完成。
    ///
    /// # 参数
    /// - `commands`: 要提交的命令缓冲区列表
    ///
    /// # 示例
    ///
    /// ```ignore
    /// cmd.begin(...);
    /// compiled_graph.execute(&cmd, resource_manager);
    /// cmd.end();
    ///
    /// let submit_info = compiled_graph.build_submit_info(&[cmd]);
    /// queue.submit(vec![submit_info], fence);
    /// ```
    pub fn build_submit_info(&self, commands: &[GfxCommandBuffer]) -> GfxSubmitInfo {
        let mut submit_info = GfxSubmitInfo::new(commands);

        // 添加 wait semaphores
        for wait in &self.wait_semaphores {
            submit_info = submit_info.wait_raw(wait.semaphore, wait.stage, wait.value);
        }

        // 添加 signal semaphores
        for signal in &self.signal_semaphores {
            submit_info = submit_info.signal_raw(signal.semaphore, signal.stage, signal.value);
        }

        submit_info
    }

    /// 获取 wait semaphores 列表（用于调试或手动构建 submit info）
    pub fn wait_semaphores(&self) -> &[RgSemaphoreInfo] {
        &self.wait_semaphores
    }

    /// 获取 signal semaphores 列表（用于调试或手动构建 submit info）
    pub fn signal_semaphores(&self) -> &[RgSemaphoreInfo] {
        &self.signal_semaphores
    }

    /// 录制 barriers
    fn record_barriers(
        &self,
        cmd: &GfxCommandBuffer,
        pass_barriers: &PassBarriers,
        resource_manager: &dyn GfxResourceAccess,
    ) {
        use truvis_gfx::commands::barrier::GfxImageBarrier;

        let image_barriers: Vec<GfxImageBarrier> = pass_barriers
            .image_barriers
            .iter()
            .map(|desc| {
                let res = self.resources.get_image(desc.handle).expect("barrier image must be registered");
                let phys_handle = res.physical_handle().expect("barrier image must be imported");
                let image = resource_manager.get_image(phys_handle).expect("barrier image disappeared during execute");
                desc.to_gfx_barrier(image.handle())
            })
            .collect();

        if !image_barriers.is_empty() {
            cmd.image_memory_barrier(vk::DependencyFlags::empty(), &image_barriers);
        }

        // Buffer 图能力暂不对外开放，当前执行路径只录制 image barrier。
    }
}

// 调试方法
impl CompiledGraph<'_> {
    /// 打印执行计划（用于调试）
    ///
    /// 输出详细的调试信息，包括 pass 顺序、image 读写和 barrier 细节。
    pub fn print_execution_plan(&self) {
        if !log::log_enabled!(log::Level::Info) {
            return;
        }

        let mut plan = String::new();
        self.write_execution_plan(&mut plan);
        log::info!("{}", plan.trim_end());
    }

    fn write_execution_plan(&self, plan: &mut String) {
        let order_text = self
            .execution_order
            .iter()
            .enumerate()
            .map(|(order, &pass_idx)| format!("[{}] {}", order + 1, self.passes[pass_idx].name))
            .join(" -> ");
        let pass_image_barriers = self.barriers.iter().map(PassBarriers::image_barrier_count).sum::<usize>();

        let _ = writeln!(plan, "RenderGraph Execution Plan");
        let _ = writeln!(plan, "summary:");
        let _ = writeln!(plan, "  passes={}, images={}", self.passes.len(), self.resources.image_count());
        let _ = writeln!(
            plan,
            "  pass barriers={} image; epilogue barriers={} image",
            pass_image_barriers,
            self.epilogue_barriers.image_barrier_count()
        );
        let _ = writeln!(
            plan,
            "  semaphores: wait={}, signal={}",
            self.wait_semaphores.len(),
            self.signal_semaphores.len()
        );
        let _ = writeln!(plan, "order: {}", if order_text.is_empty() { "<empty>" } else { order_text.as_str() });

        for (order, &pass_idx) in self.execution_order.iter().enumerate() {
            let pass = &self.passes[pass_idx];
            let barriers = &self.barriers[pass_idx];

            let _ = writeln!(plan);
            let _ = writeln!(plan, "[{}/{}] {}", order + 1, self.execution_order.len(), pass.name);
            self.write_pass_resources(plan, pass);
            self.write_barriers(plan, "barriers before pass", barriers, "  ");
        }

        let _ = writeln!(plan);
        self.write_barriers(plan, "epilogue barriers", &self.epilogue_barriers, "");
    }

    fn write_pass_resources(&self, plan: &mut String, pass: &RgPassNode<'_>) {
        if pass.image_reads.is_empty() && pass.image_writes.is_empty() {
            let _ = writeln!(plan, "  resources: none");
            return;
        }

        let _ = writeln!(plan, "  resources:");
        self.write_image_accesses(plan, "image reads", &pass.image_reads);
        self.write_image_accesses(plan, "image writes", &pass.image_writes);
    }

    fn write_image_accesses(&self, plan: &mut String, label: &str, accesses: &[(RgImageHandle, RgImageState)]) {
        if accesses.is_empty() {
            return;
        }

        let _ = writeln!(plan, "    {label}:");
        for (handle, state) in accesses {
            let _ = writeln!(
                plan,
                "      - {}: layout={:?}, stage={}, access={}",
                self.image_name(*handle),
                state.layout,
                Self::format_pipeline_stage(state.stage),
                Self::format_access_flags(state.access)
            );
        }
    }

    fn write_barriers(&self, plan: &mut String, title: &str, barriers: &PassBarriers, indent: &str) {
        if !barriers.has_barriers() {
            let _ = writeln!(plan, "{indent}{title}: none");
            return;
        }

        let _ = writeln!(plan, "{indent}{title}: {} image", barriers.image_barrier_count());

        let item_indent = format!("{indent}  ");

        for barrier in &barriers.image_barriers {
            let _ = writeln!(
                plan,
                "{}- image {}: layout={}, stage={} -> {}, access={} -> {}, aspect={:?}",
                item_indent,
                self.image_name(barrier.handle),
                Self::format_image_layout_transition(barrier.src_state.layout, barrier.dst_state.layout),
                Self::format_pipeline_stage(barrier.src_state.stage),
                Self::format_pipeline_stage(barrier.dst_state.stage),
                Self::format_access_flags(barrier.src_state.src_access()),
                Self::format_access_flags(barrier.dst_state.access),
                barrier.aspect
            );
        }
    }

    fn image_name(&self, handle: RgImageHandle) -> &str {
        self.resources.get_image(handle).map(|r| r.name.as_str()).unwrap_or("<unknown>")
    }

    fn format_image_layout_transition(src: vk::ImageLayout, dst: vk::ImageLayout) -> String {
        if src == dst { format!("{src:?}") } else { format!("{src:?} -> {dst:?}") }
    }

    /// 格式化 PipelineStageFlags2 为可读字符串
    fn format_pipeline_stage(stage: vk::PipelineStageFlags2) -> String {
        macro_rules! check_stages {
            ($($flag:ident => $name:expr),* $(,)?) => {{
                let mut stages = Vec::new();
                $(if stage.contains(vk::PipelineStageFlags2::$flag) { stages.push($name); })*
                stages
            }};
        }

        let stages = check_stages![
            TOP_OF_PIPE => "TOP_OF_PIPE",
            BOTTOM_OF_PIPE => "BOTTOM_OF_PIPE",
            VERTEX_INPUT => "VERTEX_INPUT",
            INDEX_INPUT => "INDEX_INPUT",
            DRAW_INDIRECT => "DRAW_INDIRECT",
            VERTEX_SHADER => "VERTEX_SHADER",
            FRAGMENT_SHADER => "FRAGMENT_SHADER",
            COLOR_ATTACHMENT_OUTPUT => "COLOR_ATTACHMENT_OUTPUT",
            EARLY_FRAGMENT_TESTS => "EARLY_FRAGMENT_TESTS",
            LATE_FRAGMENT_TESTS => "LATE_FRAGMENT_TESTS",
            COMPUTE_SHADER => "COMPUTE_SHADER",
            TRANSFER => "TRANSFER",
            RAY_TRACING_SHADER_KHR => "RAY_TRACING",
            ACCELERATION_STRUCTURE_BUILD_KHR => "ACCEL_BUILD",
            ALL_GRAPHICS => "ALL_GRAPHICS",
            ALL_COMMANDS => "ALL_COMMANDS",
        ];

        if stages.is_empty() { format!("{:?}", stage) } else { stages.join(" | ") }
    }

    /// 格式化 AccessFlags2 为可读字符串
    fn format_access_flags(access: vk::AccessFlags2) -> String {
        if access == vk::AccessFlags2::NONE {
            return "NONE".to_string();
        }

        macro_rules! check_access {
            ($($flag:ident => $name:expr),* $(,)?) => {{
                let mut flags = Vec::new();
                $(if access.contains(vk::AccessFlags2::$flag) { flags.push($name); })*
                flags
            }};
        }

        let flags = check_access![
            INDIRECT_COMMAND_READ => "INDIRECT_CMD_READ",
            INDEX_READ => "INDEX_READ",
            VERTEX_ATTRIBUTE_READ => "VERTEX_ATTR_READ",
            UNIFORM_READ => "UNIFORM_READ",
            SHADER_SAMPLED_READ => "SAMPLED_READ",
            SHADER_STORAGE_READ => "STORAGE_READ",
            SHADER_STORAGE_WRITE => "STORAGE_WRITE",
            SHADER_WRITE => "SHADER_WRITE",
            COLOR_ATTACHMENT_READ => "COLOR_READ",
            COLOR_ATTACHMENT_WRITE => "COLOR_WRITE",
            DEPTH_STENCIL_ATTACHMENT_READ => "DEPTH_READ",
            DEPTH_STENCIL_ATTACHMENT_WRITE => "DEPTH_WRITE",
            TRANSFER_READ => "TRANSFER_READ",
            TRANSFER_WRITE => "TRANSFER_WRITE",
            HOST_WRITE => "HOST_WRITE",
            MEMORY_READ => "MEM_READ",
            MEMORY_WRITE => "MEM_WRITE",
            ACCELERATION_STRUCTURE_READ_KHR => "ACCEL_READ",
            ACCELERATION_STRUCTURE_WRITE_KHR => "ACCEL_WRITE",
        ];

        if flags.is_empty() { format!("{:?}", access) } else { flags.join(" | ") }
    }
}
