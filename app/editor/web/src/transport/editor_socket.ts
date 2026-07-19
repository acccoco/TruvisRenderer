import { invoke, isTauri } from '@tauri-apps/api/core';

import type {
  EditorClientMessage,
  EditorNotification,
  EditorRequest,
  EditorResponse,
  EditorServerMessage,
} from '../protocol/generated';
import type { ConnectionState, EditorTransport } from './editor_transport';

const PROTOCOL_VERSION = 1;
const REQUEST_TIMEOUT_MS = 2_000;

interface PendingRequest {
  resolve: (response: EditorResponse) => void;
  reject: (error: Error) => void;
  timeout: number;
}

/**
 * 浏览器侧唯一 WebSocket owner。
 *
 * pending map 只保存通信关联与 timeout，不保存场景数据；页面场景投影由 React state 单独拥有。
 */
export class EditorSocket implements EditorTransport {
  private socket: WebSocket | null = null;
  private nextRequestId = 1;
  private readonly pending = new Map<string, PendingRequest>();
  private readonly connectionListeners = new Set<(state: ConnectionState) => void>();
  private readonly notificationListeners = new Set<(notification: EditorNotification) => void>();

  async connect(): Promise<void> {
    if (this.socket?.readyState === WebSocket.OPEN) {
      return;
    }

    this.emitConnectionState('connecting');
    const socket = new WebSocket(await this.resolveWebsocketUrl());
    this.socket = socket;

    await new Promise<void>((resolve, reject) => {
      socket.addEventListener(
        'open',
        () => {
          this.emitConnectionState('connected');
          resolve();
        },
        { once: true },
      );
      socket.addEventListener(
        'error',
        () => reject(new Error('Unable to connect to the Truvis editor server.')),
        { once: true },
      );
      socket.addEventListener('message', (event) => this.handleMessage(event));
      socket.addEventListener('close', () => this.handleClose(socket));
    });
  }

  close(): void {
    this.socket?.close();
    this.socket = null;
    this.rejectPending(new Error('Editor connection closed.'));
    this.emitConnectionState('disconnected');
  }

  request(request: EditorRequest): Promise<EditorResponse> {
    if (this.socket?.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error('Editor server is not connected.'));
    }

    const requestId = String(this.nextRequestId++);
    const message: EditorClientMessage = {
      protocol_version: PROTOCOL_VERSION,
      request_id: requestId,
      request,
    };

    return new Promise<EditorResponse>((resolve, reject) => {
      const timeout = window.setTimeout(() => {
        this.pending.delete(requestId);
        reject(new Error(`Editor request ${requestId} timed out.`));
      }, REQUEST_TIMEOUT_MS);
      this.pending.set(requestId, { resolve, reject, timeout });
      this.socket!.send(JSON.stringify(message));
    });
  }

  onConnectionState(listener: (state: ConnectionState) => void): () => void {
    this.connectionListeners.add(listener);
    return () => this.connectionListeners.delete(listener);
  }

  onNotification(listener: (notification: EditorNotification) => void): () => void {
    this.notificationListeners.add(listener);
    return () => this.notificationListeners.delete(listener);
  }

  private handleMessage(event: MessageEvent<string>): void {
    let message: EditorServerMessage;
    try {
      message = JSON.parse(event.data) as EditorServerMessage;
    } catch {
      return;
    }

    if (message.kind === 'notification') {
      for (const listener of this.notificationListeners) {
        listener(message.notification);
      }
      return;
    }

    const pending = this.pending.get(message.request_id);
    if (!pending) {
      return;
    }
    window.clearTimeout(pending.timeout);
    this.pending.delete(message.request_id);
    pending.resolve(message.response);
  }

  private async resolveWebsocketUrl(): Promise<string> {
    if (isTauri()) {
      // EditorServer 仍是现有 loopback WebSocket adapter；Tauri command 只返回
      // 实际 bind 地址，不承担材质或场景协议传输。
      return invoke<string>('editor_websocket_url');
    }
    const scheme = window.location.protocol === 'https:' ? 'wss' : 'ws';
    return `${scheme}://${window.location.host}/api/editor/v1/ws`;
  }

  private handleClose(socket: WebSocket): void {
    // React StrictMode 会执行一次 connect -> close -> connect 的探测流程。
    // 旧连接的异步 close 事件不能清空已经建立的新连接。
    if (this.socket !== socket) {
      return;
    }
    this.socket = null;
    this.rejectPending(new Error('Editor connection closed.'));
    this.emitConnectionState('disconnected');
  }

  private rejectPending(error: Error): void {
    for (const request of this.pending.values()) {
      window.clearTimeout(request.timeout);
      request.reject(error);
    }
    this.pending.clear();
  }

  private emitConnectionState(state: ConnectionState): void {
    for (const listener of this.connectionListeners) {
      listener(state);
    }
  }
}
