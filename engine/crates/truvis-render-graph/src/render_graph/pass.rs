use slotmap::SecondaryMap;

use truvis_gfx::commands::command_buffer::GfxCommandBuffer;
use truvis_gfx::resources::image::GfxImage;
use truvis_gfx::resources::image_view::GfxImageView;
use truvis_render_interface::handles::{GfxBufferHandle, GfxImageHandle, GfxImageViewHandle};

use crate::render_graph::{RgBufferHandle, RgBufferState, RgImageHandle, RgImageState};

/// Pass 执行时的上下文
///
/// 提供 Pass 执行所需的资源访问和 cmd
pub struct RgPassContext<'a> {
    /// 命令缓冲区
    pub cmd: &'a GfxCommandBuffer,

    /// 资源管理器引用（用于获取物理资源）
    pub resource_manager: &'a truvis_render_interface::gfx_resource_manager::GfxResourceManager,

    /// 物理资源查询表（编译后填充）
    pub(crate) image_handles: &'a SecondaryMap<RgImageHandle, (GfxImageHandle, GfxImageViewHandle)>,
    pub(crate) buffer_handles: &'a SecondaryMap<RgBufferHandle, GfxBufferHandle>,
}

impl<'a> RgPassContext<'a> {
    /// 获取图像的物理句柄
    #[inline]
    pub fn get_image_and_view_handle(&self, handle: RgImageHandle) -> Option<(GfxImageHandle, GfxImageViewHandle)> {
        self.image_handles.get(handle).copied()
    }

    /// 获取图像的 handle
    #[inline]
    pub fn get_image_handle(&self, handle: RgImageHandle) -> Option<GfxImageHandle> {
        self.image_handles.get(handle).map(|(h, _)| *h)
    }

    /// 获取图像的 view handle
    #[inline]
    pub fn get_image_view_handle(&self, handle: RgImageHandle) -> Option<GfxImageViewHandle> {
        self.image_handles.get(handle).map(|(_, v)| *v)
    }

    /// 获取缓冲区的物理句柄
    #[inline]
    pub fn get_buffer_handle(&self, handle: RgBufferHandle) -> Option<GfxBufferHandle> {
        self.buffer_handles.get(handle).copied()
    }

    #[inline]
    pub fn get_image_view(&self, handle: RgImageHandle) -> Option<&GfxImageView> {
        self.image_handles
            .get(handle)
            .map(|(_, view_handle)| self.resource_manager.get_image_view(*view_handle).unwrap())
    }

    #[inline]
    pub fn get_image_and_view(&self, handle: RgImageHandle) -> Option<(&GfxImage, &GfxImageView)> {
        self.image_handles.get(handle).map(|(image_handle, view_handle)| {
            let image = self.resource_manager.get_image(*image_handle).unwrap();
            let view = self.resource_manager.get_image_view(*view_handle).unwrap();
            (image, view)
        })
    }
}

/// Pass 构建器
///
/// 在 Pass 的 Setup 阶段使用，声明 Pass 的资源依赖。
pub struct RgPassBuilder {
    /// Pass 名称
    #[allow(dead_code)]
    pub(crate) name: String,

    /// 图像读取列表
    pub(crate) image_reads: Vec<(RgImageHandle, RgImageState)>,
    /// 图像写入列表
    pub(crate) image_writes: Vec<(RgImageHandle, RgImageState)>,
    /// 缓冲区读取列表
    pub(crate) buffer_reads: Vec<(RgBufferHandle, RgBufferState)>,
    /// 缓冲区写入列表
    pub(crate) buffer_writes: Vec<(RgBufferHandle, RgBufferState)>,
}

impl RgPassBuilder {
    /// 声明读取图像
    ///
    /// # 参数
    /// - `handle`: 要读取的图像句柄
    /// - `state`: 期望的图像状态（用于自动生成 barrier）
    ///
    /// # 返回
    /// 返回相同的句柄（语义上表示读取后的引用）
    #[inline]
    pub fn read_image(&mut self, handle: RgImageHandle, state: RgImageState) -> RgImageHandle {
        self.image_reads.push((handle, state));
        handle
    }

    /// 声明写入图像
    ///
    /// # 参数
    /// - `handle`: 要写入的图像句柄
    /// - `state`: 写入时的图像状态
    ///
    /// # 返回
    /// 返回相同的句柄（依赖通过 Pass 顺序确定）
    pub fn write_image(&mut self, handle: RgImageHandle, state: RgImageState) -> RgImageHandle {
        self.image_writes.push((handle, state));
        handle
    }

    /// 声明读写图像（同时读取和写入）
    ///
    /// 常用于累积操作（如 RT 累积、后处理）
    pub fn read_write_image(&mut self, handle: RgImageHandle, state: RgImageState) -> RgImageHandle {
        self.read_image(handle, state);
        self.write_image(handle, state)
    }

    /// 声明读取缓冲区
    #[inline]
    pub fn read_buffer(&mut self, handle: RgBufferHandle, state: RgBufferState) -> RgBufferHandle {
        self.buffer_reads.push((handle, state));
        handle
    }

    /// 声明写入缓冲区
    pub fn write_buffer(&mut self, handle: RgBufferHandle, state: RgBufferState) -> RgBufferHandle {
        self.buffer_writes.push((handle, state));
        handle
    }
}

/// Pass 节点数据，最后会存放于 RenderGraph 内
pub struct RgPassNode<'a> {
    /// Pass 名称
    pub name: String,

    /// 图像读取
    pub image_reads: Vec<(RgImageHandle, RgImageState)>,
    /// 图像写入
    pub image_writes: Vec<(RgImageHandle, RgImageState)>,
    /// 缓冲区读取
    pub buffer_reads: Vec<(RgBufferHandle, RgBufferState)>,
    /// 缓冲区写入
    pub buffer_writes: Vec<(RgBufferHandle, RgBufferState)>,

    /// 执行回调（类型擦除的 Pass 实现）
    pub(crate) executor: Box<dyn RgPassExecutor + 'a>,
}

/// RgPass trait
///
/// 定义渲染图中的一个 Pass
pub trait RgPass {
    /// 声明 Pass 的资源依赖
    ///
    /// 在此方法中使用 `PassBuilder` 声明读取和写入的资源。
    fn setup(&mut self, builder: &mut RgPassBuilder);

    /// 执行 Pass 的渲染逻辑
    ///
    /// 命令缓冲区已经开始录制，直接录制命令即可。
    fn execute(&self, ctx: &RgPassContext<'_>);
}

/// 基于闭包的 Pass 实现
///
/// 允许通过 lambda 快速构造 Pass，无需定义额外的结构体。
///
/// # 生命周期
///
/// - `'a`: 闭包可以捕获的外部资源的生命周期
pub struct RgLambdaPassWrapper<'a, S, E>
where
    S: FnMut(&mut RgPassBuilder) + 'a,
    E: Fn(&RgPassContext<'_>) + 'a,
{
    setup_fn: S,
    execute_fn: E,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a, S, E> RgLambdaPassWrapper<'a, S, E>
where
    S: FnMut(&mut RgPassBuilder) + 'a,
    E: Fn(&RgPassContext<'_>) + 'a,
{
    /// 创建新的 LambdaPass
    ///
    /// # 参数
    /// - `setup_fn`: setup 闭包，用于声明资源依赖
    /// - `execute_fn`: execute 闭包，用于执行渲染逻辑
    pub fn new(setup_fn: S, execute_fn: E) -> Self {
        Self {
            setup_fn,
            execute_fn,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<S, E> RgPass for RgLambdaPassWrapper<'_, S, E>
where
    S: FnMut(&mut RgPassBuilder),
    E: Fn(&RgPassContext<'_>),
{
    fn setup(&mut self, builder: &mut RgPassBuilder) {
        (self.setup_fn)(builder);
    }

    fn execute(&self, ctx: &RgPassContext<'_>) {
        (self.execute_fn)(ctx);
    }
}

/// 类型擦除的 Pass 执行器 trait
pub(crate) trait RgPassExecutor {
    /// 执行 Pass
    fn execute(&self, ctx: &RgPassContext<'_>);
}

/// 包装用户 Pass 实现的执行器
pub(crate) struct RgPassWrapper<P: RgPass> {
    pub pass: P,
}

impl<P: RgPass> RgPassExecutor for RgPassWrapper<P> {
    fn execute(&self, ctx: &RgPassContext<'_>) {
        self.pass.execute(ctx);
    }
}
