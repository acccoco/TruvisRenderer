import { invoke, isTauri } from '@tauri-apps/api/core';
import { useLayoutEffect, useRef } from 'react';

import { TruvisMark } from './icons';

interface NativeViewportRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * DOM layout 与 Windows child HWND 之间唯一的几何桥。
 *
 * DOM 只发布相对 Tauri client area 的物理像素矩形；Tauri 平台宿主负责调整
 * child HWND。材质、场景、输入和 swapchain 重建都不经过这条 IPC。
 */
export function RenderViewport() {
  const slotRef = useRef<HTMLDivElement>(null);
  const nativeDesktop = isTauri();

  useLayoutEffect(() => {
    if (!nativeDesktop) {
      return;
    }
    const slot = slotRef.current;
    if (!slot) {
      return;
    }

    let animationFrame = 0;
    let retryTimer = 0;
    let stopped = false;
    let sending = false;
    let queuedRect: NativeViewportRect | null = null;
    let lastRectKey = '';

    const flush = async () => {
      if (sending || !queuedRect || stopped) {
        return;
      }
      sending = true;
      const rect = queuedRect;
      queuedRect = null;
      try {
        await invoke('set_render_viewport_rect', { rect });
      } catch (error) {
        console.error('Failed to update native render viewport bounds.', error);
        // Tauri setup 会在显示窗口前注册 desktop state；如果 WebView 恰好更早完成
        // 首轮布局，则清除去重 key 并短暂重试，避免 child HWND 永久停在隐藏状态。
        lastRectKey = '';
        if (!stopped) {
          window.clearTimeout(retryTimer);
          retryTimer = window.setTimeout(scheduleMeasure, 100);
        }
      } finally {
        sending = false;
        if (queuedRect) {
          void flush();
        }
      }
    };

    const measure = () => {
      animationFrame = 0;
      const bounds = slot.getBoundingClientRect();
      const scale = window.devicePixelRatio || 1;
      const rect: NativeViewportRect = {
        x: Math.round(bounds.left * scale),
        y: Math.round(bounds.top * scale),
        width: Math.max(0, Math.round(bounds.width * scale)),
        height: Math.max(0, Math.round(bounds.height * scale)),
      };
      const rectKey = `${rect.x}:${rect.y}:${rect.width}:${rect.height}`;
      if (rectKey === lastRectKey) {
        return;
      }
      lastRectKey = rectKey;
      queuedRect = rect;
      void flush();
    };

    const scheduleMeasure = () => {
      if (!animationFrame) {
        animationFrame = window.requestAnimationFrame(measure);
      }
    };

    const resizeObserver = new ResizeObserver(scheduleMeasure);
    resizeObserver.observe(slot);
    resizeObserver.observe(document.documentElement);
    window.addEventListener('resize', scheduleMeasure);
    window.visualViewport?.addEventListener('resize', scheduleMeasure);
    document.fonts.ready.then(scheduleMeasure).catch(() => undefined);
    scheduleMeasure();

    return () => {
      stopped = true;
      resizeObserver.disconnect();
      window.removeEventListener('resize', scheduleMeasure);
      window.visualViewport?.removeEventListener('resize', scheduleMeasure);
      if (animationFrame) {
        window.cancelAnimationFrame(animationFrame);
      }
      window.clearTimeout(retryTimer);
      void invoke('set_render_viewport_rect', {
        rect: { x: 0, y: 0, width: 0, height: 0 } satisfies NativeViewportRect,
      }).catch(() => undefined);
    };
  }, [nativeDesktop]);

  return (
    <section className="render-viewport" aria-label="Truvis render viewport">
      <div ref={slotRef} className="render-viewport__native-slot">
        <div className="render-viewport__placeholder">
          <TruvisMark />
          <span>{nativeDesktop ? 'Starting native renderer…' : 'The native renderer is available in the Tauri desktop.'}</span>
        </div>
      </div>
    </section>
  );
}
