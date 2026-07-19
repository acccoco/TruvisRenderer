import type { CSSProperties, KeyboardEvent, PointerEvent, ReactNode } from 'react';
import { useLayoutEffect, useRef, useState } from 'react';

const DEFAULT_SCENE_PANEL_WIDTH = 260;
const DEFAULT_INSPECTOR_PANEL_WIDTH = 390;
const MIN_SCENE_PANEL_WIDTH = 220;
const MIN_INSPECTOR_PANEL_WIDTH = 320;
const MAX_SCENE_PANEL_WIDTH = 420;
const MAX_INSPECTOR_PANEL_WIDTH = 520;
const MIN_VIEWPORT_WIDTH = 320;
const RESIZER_TOTAL_WIDTH = 14;
const KEYBOARD_RESIZE_STEP = 16;

type ResizerSide = 'scene' | 'inspector';

interface PanelWidths {
  scene: number;
  inspector: number;
}

interface DragState {
  pointerId: number;
  side: ResizerSide;
  startX: number;
  startWidth: number;
  workspaceWidth: number;
}

interface EditorWorkspaceProps {
  scenePanel: ReactNode;
  viewport: ReactNode;
  inspector: ReactNode;
}

function constrainPanelWidths(
  requested: PanelWidths,
  workspaceWidth: number,
  resizedSide?: ResizerSide,
): PanelWidths {
  let scene = Math.min(MAX_SCENE_PANEL_WIDTH, Math.max(MIN_SCENE_PANEL_WIDTH, requested.scene));
  let inspector = Math.min(MAX_INSPECTOR_PANEL_WIDTH, Math.max(MIN_INSPECTOR_PANEL_WIDTH, requested.inspector));
  let overflow = scene + inspector - Math.max(
    MIN_SCENE_PANEL_WIDTH + MIN_INSPECTOR_PANEL_WIDTH,
    workspaceWidth - MIN_VIEWPORT_WIDTH - RESIZER_TOTAL_WIDTH,
  );

  // 用户拖动时只约束正在调整的一侧；窗口整体收窄时才依次回收两侧额外宽度。
  if (overflow > 0) {
    if (resizedSide === 'scene') {
      scene -= Math.min(overflow, scene - MIN_SCENE_PANEL_WIDTH);
    } else {
      const inspectorReduction = Math.min(overflow, inspector - MIN_INSPECTOR_PANEL_WIDTH);
      inspector -= inspectorReduction;
      overflow -= inspectorReduction;
      scene -= Math.min(overflow, scene - MIN_SCENE_PANEL_WIDTH);
    }
  }

  return { scene, inspector };
}

/**
 * Editor 三列 DOM 布局及其 resize 状态的唯一 owner。
 *
 * 这里只改变左右面板的 CSS 宽度；中央区域变化后，RenderViewport 已有的
 * ResizeObserver 会把最新 DOM rect 同步给 native child HWND。
 */
export function EditorWorkspace({ scenePanel, viewport, inspector }: EditorWorkspaceProps) {
  const workspaceRef = useRef<HTMLElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const [activeSide, setActiveSide] = useState<ResizerSide | null>(null);
  const [widths, setWidths] = useState<PanelWidths>({
    scene: DEFAULT_SCENE_PANEL_WIDTH,
    inspector: DEFAULT_INSPECTOR_PANEL_WIDTH,
  });

  useLayoutEffect(() => {
    const workspace = workspaceRef.current;
    if (!workspace) {
      return;
    }

    const observer = new ResizeObserver(([entry]) => {
      if (!entry) {
        return;
      }
      setWidths((current) => {
        const next = constrainPanelWidths(current, entry.contentRect.width);
        return next.scene === current.scene && next.inspector === current.inspector ? current : next;
      });
    });
    observer.observe(workspace);
    return () => observer.disconnect();
  }, []);

  const beginResize = (side: ResizerSide, event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return;
    }
    const workspace = workspaceRef.current;
    if (!workspace) {
      return;
    }

    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = {
      pointerId: event.pointerId,
      side,
      startX: event.clientX,
      startWidth: widths[side],
      workspaceWidth: workspace.clientWidth,
    };
    setActiveSide(side);
  };

  const continueResize = (event: PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) {
      return;
    }

    const delta = event.clientX - drag.startX;
    const requestedWidth = drag.startWidth + (drag.side === 'scene' ? delta : -delta);
    setWidths((current) => constrainPanelWidths(
      { ...current, [drag.side]: requestedWidth },
      drag.workspaceWidth,
      drag.side,
    ));
  };

  const finishResize = (event: PointerEvent<HTMLDivElement>) => {
    if (dragRef.current?.pointerId !== event.pointerId) {
      return;
    }
    dragRef.current = null;
    setActiveSide(null);
  };

  const resizeWithKeyboard = (side: ResizerSide, event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') {
      return;
    }
    const workspace = workspaceRef.current;
    if (!workspace) {
      return;
    }

    event.preventDefault();
    const direction = event.key === 'ArrowRight' ? 1 : -1;
    const delta = side === 'scene' ? direction * KEYBOARD_RESIZE_STEP : -direction * KEYBOARD_RESIZE_STEP;
    setWidths((current) => constrainPanelWidths(
      { ...current, [side]: current[side] + delta },
      workspace.clientWidth,
      side,
    ));
  };

  const workspaceStyle = {
    '--scene-panel-width': `${widths.scene}px`,
    '--inspector-panel-width': `${widths.inspector}px`,
  } as CSSProperties;

  return (
    <main
      ref={workspaceRef}
      className={`editor-workspace${activeSide ? ' editor-workspace--resizing' : ''}`}
      style={workspaceStyle}
    >
      {scenePanel}
      <div
        className={`workspace-resizer${activeSide === 'scene' ? ' workspace-resizer--active' : ''}`}
        role="separator"
        aria-label="Resize scene panel and render viewport"
        aria-orientation="vertical"
        aria-valuemin={MIN_SCENE_PANEL_WIDTH}
        aria-valuemax={MAX_SCENE_PANEL_WIDTH}
        aria-valuenow={Math.round(widths.scene)}
        tabIndex={0}
        onKeyDown={(event) => resizeWithKeyboard('scene', event)}
        onPointerDown={(event) => beginResize('scene', event)}
        onPointerMove={continueResize}
        onPointerUp={finishResize}
        onPointerCancel={finishResize}
        onLostPointerCapture={finishResize}
      />
      {viewport}
      <div
        className={`workspace-resizer${activeSide === 'inspector' ? ' workspace-resizer--active' : ''}`}
        role="separator"
        aria-label="Resize render viewport and inspector panel"
        aria-orientation="vertical"
        aria-valuemin={MIN_INSPECTOR_PANEL_WIDTH}
        aria-valuemax={MAX_INSPECTOR_PANEL_WIDTH}
        aria-valuenow={Math.round(widths.inspector)}
        tabIndex={0}
        onKeyDown={(event) => resizeWithKeyboard('inspector', event)}
        onPointerDown={(event) => beginResize('inspector', event)}
        onPointerMove={continueResize}
        onPointerUp={finishResize}
        onPointerCancel={finishResize}
        onLostPointerCapture={finishResize}
      />
      {inspector}
    </main>
  );
}
