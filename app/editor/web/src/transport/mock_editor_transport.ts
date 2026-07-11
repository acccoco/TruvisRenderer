import type {
  EditorNotification,
  EditorRequest,
  EditorResponse,
  MaterialDto,
  SceneObjectSummary,
  SelectionDto,
} from '../protocol/generated';
import type { ConnectionState, EditorTransport } from './editor_transport';

const MOCK_INSTANCE_ID = 'instance:00000001000003ec';
const MOCK_MATERIAL_ID = 'material:000000010000da0c';

/** 仅供 Vite 开发视觉验收使用，不参与生产 Server/World 语义。 */
export class MockEditorTransport implements EditorTransport {
  private sceneVersion = 24;
  private material: MaterialDto = {
    id: MOCK_MATERIAL_ID,
    name: 'Crate_Paint_Olive',
    base_color: [0.317, 0.396, 0.305, 1],
    metallic: 0.12,
    roughness: 0.58,
    class: { kind: 'surface' },
    coverage: { kind: 'opaque' },
    diffuse_texture: 'texture:0000000100000041',
    normal_texture: 'texture:0000000100000042',
  };
  private readonly selection: SelectionDto = {
    instance_id: MOCK_INSTANCE_ID,
    submesh_index: 1,
    material_id: MOCK_MATERIAL_ID,
  };
  private readonly objects: SceneObjectSummary[] = Array.from({ length: 18 }, (_, index) => ({
    instance_id: index === 3 ? MOCK_INSTANCE_ID : `instance:000000010000${(1001 + index).toString(16).padStart(4, '0')}`,
    material_count: index % 4 === 3 ? 3 : (index % 2) + 1,
  }));
  private readonly connectionListeners = new Set<(state: ConnectionState) => void>();
  private readonly notificationListeners = new Set<(notification: EditorNotification) => void>();

  async connect(): Promise<void> {
    this.emitConnectionState('connecting');
    await new Promise((resolve) => window.setTimeout(resolve, 80));
    this.emitConnectionState('connected');
  }

  close(): void {
    this.emitConnectionState('disconnected');
  }

  async request(request: EditorRequest): Promise<EditorResponse> {
    await new Promise((resolve) => window.setTimeout(resolve, 34));
    if (request.category === 'command') {
      for (const [key, value] of Object.entries(request.payload.patch)) {
        if (value !== null) {
          Object.assign(this.material, { [key]: value });
        }
      }
      this.sceneVersion += 1;
      const version = String(this.sceneVersion);
      this.emitNotification({ type: 'scene_version_changed', payload: version });
      return {
        type: 'command_applied',
        payload: { scene_version: version, material: { ...this.material } },
      };
    }

    switch (request.payload.type) {
      case 'get_capabilities':
        return {
          type: 'capabilities',
          payload: {
            protocol_version: 1,
            max_scene_page_size: 256,
            editable_material_fields: ['name', 'base_color', 'metallic', 'roughness', 'class', 'coverage'],
          },
        };
      case 'get_scene_version':
        return { type: 'scene_version', payload: String(this.sceneVersion) };
      case 'get_selection':
        return { type: 'selection', payload: this.selection };
      case 'get_material':
        return { type: 'material', payload: { ...this.material } };
      case 'get_scene_objects': {
        const offset = request.payload.offset;
        const limit = request.payload.limit || 128;
        const objects = this.objects.slice(offset, offset + limit);
        return {
          type: 'scene_objects',
          payload: {
            scene_version: String(this.sceneVersion),
            objects,
            next_offset: offset + objects.length < this.objects.length ? offset + objects.length : null,
          },
        };
      }
    }
  }

  onConnectionState(listener: (state: ConnectionState) => void): () => void {
    this.connectionListeners.add(listener);
    return () => this.connectionListeners.delete(listener);
  }

  onNotification(listener: (notification: EditorNotification) => void): () => void {
    this.notificationListeners.add(listener);
    return () => this.notificationListeners.delete(listener);
  }

  private emitConnectionState(state: ConnectionState): void {
    for (const listener of this.connectionListeners) {
      listener(state);
    }
  }

  private emitNotification(notification: EditorNotification): void {
    for (const listener of this.notificationListeners) {
      listener(notification);
    }
  }
}
