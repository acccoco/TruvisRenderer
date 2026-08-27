import type { EditorTransport } from './editor_transport';
import { MockEditorTransport } from './mock_editor_transport';
import { TauriEditorTransport } from './tauri_editor_transport';

export function createEditorTransport(): EditorTransport {
  const useMock = import.meta.env.DEV && new URLSearchParams(window.location.search).get('mock') === '1';
  return useMock ? new MockEditorTransport() : new TauriEditorTransport();
}
