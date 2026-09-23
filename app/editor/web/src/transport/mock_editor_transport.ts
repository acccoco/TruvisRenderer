import type {
  LightDetailsDto, LightPatch, EnvironmentDetailsDto,
  EditorNotification,
  EditorRequest,
  EditorResponse,
  InstanceDetailsDto,
  MaterialDto,
  SceneObjectSummary,
  SelectionDto,
} from '../protocol/generated';
import type { EditorBackendState, EditorTransport } from './editor_transport';

const MOCK_INSTANCE_ID = 'instance:00000001000003ec';
const MOCK_MATERIAL_ID = 'material:000000010000da0c';

/** 仅供 Vite 开发视觉验收使用，不参与生产 Tauri IPC/World 语义。 */
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
    textures: [
      { texture: 'texture:0000000100000041', mapping: { tex_coord: 0, offset: [0, 0], rotation: 0, scale: [1, 1], wrap: [0, 0], filter: 1 } },
      null, null, null,
    ],
    normal_scale: 1,
    emissive_factor: [0, 0, 0],
  };
  private readonly selection: SelectionDto = {
    type: 'submesh',
    instance_id: MOCK_INSTANCE_ID,
    submesh_index: 1,
    material_id: MOCK_MATERIAL_ID,
  };
  private readonly instances: Extract<SceneObjectSummary, { type: 'instance' }>[] = Array.from({ length: 18 }, (_, index) => ({
    type: 'instance',
    instance_id: index === 3 ? MOCK_INSTANCE_ID : `instance:000000010000${(1001 + index).toString(16).padStart(4, '0')}`,
    name: index === 3 ? 'Sponza_Curtain_West' : `Sponza_Instance_${String(index + 1).padStart(2, '0')}`,
    material_count: index % 4 === 3 ? 3 : (index % 2) + 1,
  }));
  private readonly lights: LightDetailsDto[] = [
    { light_id: 'point:0000000100000001', scene_version: '24', position: [0, 1, 0], radiance: [8, 6, 4], parameters: { kind: 'point' } },
    { light_id: 'spot:0000000100000001', scene_version: '24', position: [2, 3, 0], radiance: [10, 10, 10], parameters: { kind: 'spot', direction: [0, -1, 0], inner_angle_degrees: 20, outer_angle_degrees: 40 } },
    { light_id: 'area:0000000100000001', scene_version: '24', position: [0, 4, 0], radiance: [3, 3, 3], parameters: { kind: 'area', shape: { rotation_degrees: [90, 0, 0], width: 2, height: 3 } } },
  ];
  private environment: EnvironmentDetailsDto = { scene_version: '24', enabled: true, brightness: 1, texture_id: null, file_name: null, load_state: 'unset' };
  private get objects(): SceneObjectSummary[] {
    return [...this.instances, ...this.lights.map((light): SceneObjectSummary => ({
      type: light.parameters.kind, light_id: light.light_id, name: `${light.parameters.kind} Light 1`,
    })), { type: 'environment', name: 'Environment / HDRI (not set)' }];
  }

  private readonly stateListeners = new Set<(state: EditorBackendState) => void>();
  private readonly notificationListeners = new Set<(notification: EditorNotification) => void>();

  async connect(): Promise<void> {
    this.emitState('starting');
    await new Promise((resolve) => window.setTimeout(resolve, 80));
    this.emitState('ready');
  }

  close(): void {
    this.emitState('unavailable');
  }

  async request(request: EditorRequest): Promise<EditorResponse> {
    await new Promise((resolve) => window.setTimeout(resolve, 34));
    if (request.category === 'command') {
      if (request.payload.type === 'update_light') return this.updateLight(request.payload.light_id, request.payload.patch);
      if (request.payload.type === 'update_environment') {
        const patch = request.payload.patch;
        if (patch.brightness !== null && (!Number.isFinite(patch.brightness) || patch.brightness < 0))
          return { type: 'error', payload: { code: 'invalid_request', message: 'Brightness must be finite and nonnegative' } };
        const next = { ...this.environment, enabled: patch.enabled ?? this.environment.enabled, brightness: patch.brightness ?? this.environment.brightness };
        if (JSON.stringify(next) !== JSON.stringify(this.environment)) next.scene_version = this.changed();
        next.scene_version = String(this.sceneVersion);
        this.environment = next;
        return { type: 'environment_applied', payload: structuredClone(next) };
      }

      this.material.id = request.payload.material_id;
      const { texture_mappings, ...values } = request.payload.patch;
      for (const [key, value] of Object.entries(values)) {
        if (value !== null) {
          Object.assign(this.material, { [key]: value });
        }
      }
      for (const { channel, mapping } of texture_mappings ?? []) {
        const slot = this.material.textures[channel];
        if (slot) {
          this.material.textures = this.material.textures.map((current, index) =>
            index === channel ? { texture: slot.texture, mapping: structuredClone(mapping) } : current,
          ) as MaterialDto['textures'];
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
      case 'get_scene_version':
        return { type: 'scene_version', payload: String(this.sceneVersion) };
      case 'get_environment': return { type: 'environment', payload: { ...this.environment, scene_version: String(this.sceneVersion) } };
      case 'get_light_details': {
        const id = request.payload.light_id;
        const light = this.lights.find((entry) => entry.light_id === id);
        return light ? { type: 'light_details', payload: { ...structuredClone(light), scene_version: String(this.sceneVersion) } }
          : { type: 'error', payload: { code: 'stale_object', message: 'Light no longer exists' } };
      }
      case 'get_selection':
        return { type: 'selection', payload: this.selection };
      case 'get_instance_details': {
        const instanceId = request.payload.instance_id;
        const object = this.instances.find((candidate) => candidate.instance_id === instanceId);
        if (!object) {
          return {
            type: 'error',
            payload: { code: 'stale_object', message: 'instance ID is no longer valid' },
          };
        }
        const materials = Array.from({ length: object.material_count }, (_, submeshIndex) => ({
          submesh_index: submeshIndex,
          material_id: submeshIndex === 1 ? MOCK_MATERIAL_ID : `material:000000010000da${(10 + submeshIndex).toString(16)}`,
          name: submeshIndex === 1 ? this.material.name : `Sponza_Material_${submeshIndex + 1}`,
        }));
        const details: InstanceDetailsDto = {
          scene_version: String(this.sceneVersion),
          instance_id: object.instance_id,
          name: object.name,
          // 最后一项覆盖不可分解状态；mock 只提供投影，不执行矩阵运算。
          transform: object === this.instances[this.instances.length - 1] ? null : {
            location: [124.5, 32, -48.25],
            rotation_degrees: [30, 0, 0],
            scale: [1, 1, 1],
          },
          mesh: {
            mesh_id: 'mesh:000000010000a410',
            name: 'Sponza_Curtain_Mesh',
          },
          materials,
        };
        return { type: 'instance_details', payload: details };
      }
      case 'get_material':
        return { type: 'material', payload: { ...structuredClone(this.material), id: request.payload.material_id } };
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

  private changed(): string {
    const version = String(++this.sceneVersion);
    this.emitNotification({ type: 'scene_version_changed', payload: version });
    return version;
  }

  private updateLight(id: string, patch: LightPatch): EditorResponse {
    const index = this.lights.findIndex((light) => light.light_id === id);
    if (index < 0) return { type: 'error', payload: { code: 'stale_object', message: 'Light no longer exists' } };
    const next = structuredClone(this.lights[index]);
    const p = next.parameters;
    if (patch.position) next.position = patch.position;
    if (patch.radiance) next.radiance = patch.radiance;
    if (p.kind === 'spot') {
      if (patch.direction) {
        const length = Math.hypot(...patch.direction);
        p.direction = patch.direction.map((v) => v / length) as [number, number, number];
      }
      p.inner_angle_degrees = patch.inner_angle_degrees ?? p.inner_angle_degrees;
      p.outer_angle_degrees = patch.outer_angle_degrees ?? p.outer_angle_degrees;
    }
    if (p.kind === 'area' && p.shape) {
      p.shape.rotation_degrees = patch.rotation_degrees ?? p.shape.rotation_degrees;
      p.shape.width = patch.width ?? p.shape.width;
      p.shape.height = patch.height ?? p.shape.height;
    }
    const invalid = next.position.some((v) => !Number.isFinite(v)) || next.radiance.some((v) => !Number.isFinite(v) || v < 0)
      || (p.kind === 'spot' && (p.direction.some((v) => !Number.isFinite(v)) || p.inner_angle_degrees < 0 || p.outer_angle_degrees > 180 || p.inner_angle_degrees > p.outer_angle_degrees))
      || (p.kind === 'area' && p.shape && (p.shape.width <= 0 || p.shape.height <= 0));
    if (invalid) return { type: 'error', payload: { code: 'invalid_request', message: 'Invalid light parameters' } };
    if (JSON.stringify(next) !== JSON.stringify(this.lights[index])) next.scene_version = this.changed();
    next.scene_version = String(this.sceneVersion);
    this.lights[index] = next;
    return { type: 'light_applied', payload: structuredClone(next) };
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

  private emitNotification(notification: EditorNotification): void {
    for (const listener of this.notificationListeners) {
      listener(notification);
    }
  }
}
