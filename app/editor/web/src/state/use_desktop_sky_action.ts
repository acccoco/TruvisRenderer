import { invoke, isTauri } from '@tauri-apps/api/core';
import { useCallback, useRef, useState } from 'react';

type SelectHdriResult =
  | { status: 'cancelled' }
  | {
      status: 'accepted';
      file_name: string;
    };

/**
 * Tauri-only HDRI 文件选择动作的页面状态。
 *
 * 该状态与通用 Editor DTO 分离：文件系统授权和原生 dialog 属于桌面壳，
 * Web 只接收文件名与 CPU scene 接受结果，不持有或传输本机路径。
 */
export interface DesktopSkyActionState {
  supported: boolean;
  selecting: boolean;
  lastRequestedFile: string | null;
  error: string | null;
}

export interface DesktopSkyAction {
  state: DesktopSkyActionState;
  chooseHdri(): Promise<void>;
}

export function useDesktopSkyAction(): DesktopSkyAction {
  const supported = isTauri();
  const selectingRef = useRef(false);
  const [state, setState] = useState<DesktopSkyActionState>({
    supported,
    selecting: false,
    lastRequestedFile: null,
    error: null,
  });

  const chooseHdri = useCallback(async () => {
    if (!supported || selectingRef.current) {
      return;
    }

    selectingRef.current = true;
    setState((current) => ({ ...current, selecting: true, error: null }));
    try {
      const result = await invoke<SelectHdriResult>('select_hdri');
      if (result.status === 'accepted') {
        setState((current) => ({
          ...current,
          lastRequestedFile: result.file_name,
        }));
      }
    } catch (error) {
      setState((current) => ({
        ...current,
        error: error instanceof Error ? error.message : String(error),
      }));
    } finally {
      selectingRef.current = false;
      setState((current) => ({ ...current, selecting: false }));
    }
  }, [supported]);

  return { state, chooseHdri };
}
