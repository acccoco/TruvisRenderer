import type {
  EditorNotification,
  EditorRequest,
  EditorResponse,
} from '../protocol/generated';

export type ConnectionState = 'connecting' | 'connected' | 'disconnected';

export interface EditorTransport {
  connect(): Promise<void>;
  close(): void;
  request(request: EditorRequest): Promise<EditorResponse>;
  onConnectionState(listener: (state: ConnectionState) => void): () => void;
  onNotification(listener: (notification: EditorNotification) => void): () => void;
}
