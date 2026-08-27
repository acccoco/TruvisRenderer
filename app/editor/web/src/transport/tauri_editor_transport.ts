import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type { EditorNotification, EditorRequest, EditorResponse } from '../protocol/generated';
import type { EditorBackendState, EditorTransport } from './editor_transport';

const EDITOR_NOTIFICATION_EVENT = 'editor-notification';

/** Tauri WebView 中唯一的真实 Editor transport。 */
export class TauriEditorTransport implements EditorTransport {
  private generation = 0;
  private unlisten: UnlistenFn | null = null;
  private readonly stateListeners = new Set<(state: EditorBackendState) => void>();
  private readonly notificationListeners = new Set<(notification: EditorNotification) => void>();

  async connect(): Promise<void> {
    const generation = ++this.generation;
    this.emitState('starting');
    if (!isTauri()) {
      this.emitState('unavailable');
      throw new Error('The real Truvis editor backend is only available inside the Tauri desktop app.');
    }

    const unlisten = await listen<EditorNotification>(EDITOR_NOTIFICATION_EVENT, (event) => {
      if (generation !== this.generation) {
        return;
      }
      for (const listener of this.notificationListeners) {
        listener(event.payload);
      }
    });

    // React StrictMode 可能在 listen 完成前执行 cleanup；旧 generation 必须立即撤销，
    // 不能覆盖后一次 connect 已经建立的 listener。
    if (generation !== this.generation) {
      unlisten();
      return;
    }
    this.unlisten?.();
    this.unlisten = unlisten;
    this.emitState('ready');
  }

  close(): void {
    this.generation += 1;
    this.unlisten?.();
    this.unlisten = null;
    this.emitState('unavailable');
  }

  request(request: EditorRequest): Promise<EditorResponse> {
    if (!isTauri()) {
      return Promise.reject(new Error('The real Truvis editor backend is only available inside the Tauri desktop app.'));
    }
    return invoke<EditorResponse>('editor_request', { request });
  }

  onState(listener: (state: EditorBackendState) => void): () => void {
    this.stateListeners.add(listener);
    return () => this.stateListeners.delete(listener);
  }

  onNotification(listener: (notification: EditorNotification) => void): () => void {
    this.notificationListeners.add(listener);
    return () => this.notificationListeners.delete(listener);
  }

  private emitState(state: EditorBackendState): void {
    for (const listener of this.stateListeners) {
      listener(state);
    }
  }
}
