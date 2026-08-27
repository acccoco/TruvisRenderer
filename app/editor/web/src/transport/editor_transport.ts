import type {
  EditorNotification,
  EditorRequest,
  EditorResponse,
} from '../protocol/generated';

export type EditorBackendState = 'starting' | 'ready' | 'unavailable';

export interface EditorTransport {
  connect(): Promise<void>;
  close(): void;
  request(request: EditorRequest): Promise<EditorResponse>;
  onState(listener: (state: EditorBackendState) => void): () => void;
  onNotification(listener: (notification: EditorNotification) => void): () => void;
}
