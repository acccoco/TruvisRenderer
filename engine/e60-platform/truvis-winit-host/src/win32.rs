use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, Win32WindowHandle, WindowsDisplayHandle,
};
use winit::window::Window;

use truvis_app_frame::RenderThreadInit;

/// 可跨线程传递的 Windows surface 参数；只保存 raw-window-handle 明确标记为 Send 的具体变体。
///
/// backend 在线程所属的 winit Window 上提取句柄；目标 RenderThread 再重建通用 raw enum。
/// 句柄指向的窗口仍由窗口 owner 持有，且必须存活到 RenderThread::join 返回。
pub(crate) struct Win32RenderSurface {
    display: WindowsDisplayHandle,
    window: Win32WindowHandle,
    scale_factor: f64,
    initial_size: [u32; 2],
}

impl Win32RenderSurface {
    pub(crate) fn from_window(window: &Window) -> Result<Self, String> {
        let display =
            match window.display_handle().map_err(|error| format!("failed to get display handle: {error}"))?.as_raw() {
                RawDisplayHandle::Windows(handle) => handle,
                other => return Err(format!("Windows render host received non-Windows display handle: {other:?}")),
            };
        let raw_window =
            window.window_handle().map_err(|error| format!("failed to get window handle: {error}"))?.as_raw();
        let window_handle = Self::require_window_handle(raw_window)?;
        let size = window.inner_size();

        Ok(Self {
            display,
            window: window_handle,
            scale_factor: window.scale_factor(),
            initial_size: [size.width, size.height],
        })
    }

    pub(crate) fn require_window_handle(handle: RawWindowHandle) -> Result<Win32WindowHandle, String> {
        match handle {
            RawWindowHandle::Win32(handle) => Ok(handle),
            other => Err(format!("Windows render host received non-Win32 window handle: {other:?}")),
        }
    }

    pub(crate) fn initial_size(&self) -> [u32; 2] {
        self.initial_size
    }

    /// 只在目标 RenderThread 内重建 !Send raw handle enum。
    pub(crate) fn into_render_thread_init(self, initial_size: [u32; 2]) -> RenderThreadInit {
        RenderThreadInit {
            raw_display: RawDisplayHandle::Windows(self.display),
            raw_window: RawWindowHandle::Win32(self.window),
            scale_factor: self.scale_factor,
            initial_size,
        }
    }
}
