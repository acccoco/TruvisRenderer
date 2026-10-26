//! NR 最小 C ABI 的单向 consumer。所有 GPU 资源、提交和同步均由调用方拥有。
//! 会话不得跨线程；release_feature 与 shutdown 显式分开，Drop 不操作全局 NGX。

use std::{ffi::CStr, fmt, marker::PhantomData, ptr, rc::Rc};

pub mod ffi {
    include!(env!("TRUVIS_NR_BINDINGS_RS"));
}
const _: &str = env!("TRUVIS_NR_BINDINGS_HASH");
const _: &str = include_str!(env!("TRUVIS_NR_BINDINGS_HASH_FILE"));

pub use ffi::root as raw;
pub use raw::{
    TruvixxNrCapabilities as Capabilities, TruvixxNrEvaluateDesc as EvaluateDesc, TruvixxNrImage as Image,
    TruvixxNrInitDesc as InitDesc,
};

/// 保留 SDK 原始错误与 wrapper 错误；能力参数各自的查询返回码在 Capabilities 中。
#[derive(Debug, Clone, Copy)]
pub struct NrError(pub raw::TruvixxNrStatus);

impl NrError {
    fn check(status: raw::TruvixxNrStatus) -> Result<(), Self> {
        if status.error == 0 && status.ngx_result == 1 { Ok(()) } else { Err(Self(status)) }
    }
}
impl fmt::Display for NrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NR stage={} NGX={:#x} wrapper={}", self.0.stage, self.0.ngx_result, self.0.error)
    }
}
impl std::error::Error for NrError {}

/// 一个进程只能持有一个 NR 会话；Vulkan root 必须存活至显式 shutdown 成功。
pub struct NrSession {
    handle: *mut raw::TruvixxNrSession,
    _not_send_sync: PhantomData<Rc<()>>,
}

impl NrSession {
    /// 查询并复制扩展名，device=false 时允许 Vulkan root 句柄为空。
    ///
    /// # Safety
    /// desc 的 UTF-16 字符串须有效且以零结尾；device=true 时 root 句柄须有效。
    /// 调用方在同一线程串行协调本查询与其它 NGX/Streamline 调用。
    pub unsafe fn extensions(desc: &InitDesc, device: bool) -> Result<Vec<String>, NrError> {
        let mut count = 0;
        NrError::check(unsafe { raw::truvixx_nr_extensions(desc, u32::from(device), ptr::null_mut(), &mut count) })?;
        let mut names: Vec<raw::TruvixxNrExtension> = Vec::with_capacity(count as usize);
        names.resize_with(count as usize, || unsafe { std::mem::zeroed() });
        NrError::check(unsafe { raw::truvixx_nr_extensions(desc, u32::from(device), names.as_mut_ptr(), &mut count) })?;
        Ok(names
            .iter()
            .take(count as usize)
            .map(|name| unsafe { CStr::from_ptr(name.name.as_ptr()) }.to_string_lossy().into_owned())
            .collect())
    }

    /// Streamline 首次 NGX 初始化前必须已经部署 NR DLL；成功后必须显式 shutdown。
    ///
    /// # Safety
    /// desc 的句柄与函数指针来自同一有效 Vulkan/interposer 链路，features 声明必须属实。
    /// 所有路径有效且以零结尾；调用者保持 Vulkan root 存活并串行协调 Streamline/NGX。
    pub unsafe fn init(desc: &InitDesc) -> Result<Self, NrError> {
        let mut handle = ptr::null_mut();
        NrError::check(unsafe { raw::truvixx_nr_init(desc, &mut handle) })?;
        Ok(Self {
            handle,
            _not_send_sync: PhantomData,
        })
    }

    /// 调用成功只表示获得查询结果；各参数的 ngx_result 必须分别检查。
    pub fn capabilities(&mut self) -> Result<Capabilities, NrError> {
        let mut caps = unsafe { std::mem::zeroed() };
        NrError::check(unsafe { raw::truvixx_nr_capabilities(self.handle, &mut caps) })?;
        Ok(caps)
    }

    /// 只在 command buffer 中录制创建工作，调用方随后负责提交并确认 GPU 完成。
    ///
    /// # Safety
    /// command_buffer 属于初始化时的 device，处于可录制状态且不在 render pass 内。
    pub unsafe fn create_feature(&mut self, command_buffer: u64, width: u32, height: u32) -> Result<(), NrError> {
        NrError::check(unsafe { raw::truvixx_nr_create(self.handle, command_buffer, width, height) })
    }

    /// 只录制 NR 命令；失败输出不得消费。
    ///
    /// # Safety
    /// command_buffer 可录制且来自同一 device；图像描述与实际资源一致，全部为 GENERAL。
    /// 同帧颜色/深度/MV/reset 契约成立，前后 barrier 与 GPU 生命周期由调用方保证。
    pub unsafe fn evaluate(&mut self, command_buffer: u64, desc: &EvaluateDesc) -> Result<(), NrError> {
        NrError::check(unsafe { raw::truvixx_nr_evaluate(self.handle, command_buffer, desc) })
    }

    /// 关闭效果时只释放 feature，保留会话供后续重建。
    ///
    /// # Safety
    /// 所有引用该 feature 和相关图像的 GPU 工作已完成，没有尚待提交的旧命令。
    pub unsafe fn release_feature(&mut self) -> Result<(), NrError> {
        NrError::check(unsafe { raw::truvixx_nr_release(self.handle) })
    }

    /// 成功后句柄置空；失败保留会话，供调用方记录或显式处理。
    ///
    /// # Safety
    /// NR 与 SR/RR 的 feature 均已释放、GPU 已完成；Streamline 和 Vulkan root 仍存活。
    /// 成功后再关闭 Streamline，最后销毁 Vulkan root。
    pub unsafe fn shutdown(&mut self) -> Result<(), NrError> {
        NrError::check(unsafe { raw::truvixx_nr_shutdown(&mut self.handle) })
    }
}

impl Drop for NrSession {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            log::error!("NR session dropped without explicit shutdown; SDK state intentionally not destroyed.");
            debug_assert!(std::thread::panicking(), "NR session requires explicit shutdown");
        }
    }
}
