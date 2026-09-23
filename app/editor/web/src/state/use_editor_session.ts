import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  EditorCommand, EditorNotification, EditorQuery, EditorRequest, EditorResponse,
  EnvironmentDetailsDto, EnvironmentPatch, InstanceDetailsDto, LightDetailsDto, LightPatch,
  MaterialDto, MaterialPatch, SceneObjectSummary, SelectionDto,
} from '../protocol/generated';
import { createEditorTransport } from '../transport/create_editor_transport';
import type { EditorBackendState } from '../transport/editor_transport';

export type ObjectDetailsStatus = 'idle' | 'loading' | 'ready' | 'stale' | 'error';
export type InspectedObject = { type: 'instance'; instance_id: string } | { type: 'light'; light_id: string } | { type: 'environment' };
export type InspectorDetails = { type: 'instance'; value: InstanceDetailsDto } | { type: 'light'; value: LightDetailsDto } | { type: 'environment'; value: EnvironmentDetailsDto };

interface EditorSessionState {
  backendState: EditorBackendState;
  sceneVersion: string;
  selection: SelectionDto | null;
  objects: SceneObjectSummary[];
  inspectedObject: InspectedObject | null;
  details: InspectorDetails | null;
  detailsStatus: ObjectDetailsStatus;
  materialId: string | null;
  material: MaterialDto | null;
  materialDraft: Partial<MaterialDto>;
  lightDraft: Partial<LightPatch>;
  environmentDraft: Partial<EnvironmentPatch>;
  pendingRequests: number;
  lastRequestMs: number | null;
  error: string | null;
}

class EditorResponseError extends Error {
  constructor(readonly code: string, message: string) { super(message); }
}

/** 当前检查对象、在途请求和草稿的唯一协调入口；World 数据只作为可丢弃投影。 */
export function useEditorSession() {
  const [transport] = useState(createEditorTransport);
  const [state, setState] = useState<EditorSessionState>({
    backendState: 'unavailable', sceneVersion: '0', selection: null, objects: [], inspectedObject: null,
    details: null, detailsStatus: 'idle', materialId: null, material: null,
    materialDraft: {}, lightDraft: {}, environmentDraft: {}, pendingRequests: 0, lastRequestMs: null, error: null,
  });
  const current = useRef(state);
  const active = useRef(true);
  const epoch = useRef(0);
  const detailsSequence = useRef(0);
  const materialSequence = useRef(0);
  const listSequence = useRef(0);
  const objectsVersion = useRef('0');
  const materialEpoch = useRef(0);
  const selectionSequence = useRef(0);
  const editRevision = useRef(0);
  const fieldRevisions = useRef<Record<string, number>>({});
  const commandTail = useRef<Promise<void>>(Promise.resolve());

  // 同步更新 ref，让同一事件内的提交和异步回包看到一致身份；不等待 React effect。
  const change = useCallback((patch: Partial<EditorSessionState>) => {
    if (!active.current) return;
    current.current = { ...current.current, ...patch };
    setState(current.current);
  }, []);

  const observeVersion = useCallback((version: string) => {
    if (BigInt(version) > BigInt(current.current.sceneVersion)) change({ sceneVersion: version });
  }, [change]);

  const request = useCallback(async (value: EditorRequest): Promise<EditorResponse> => {
    const started = performance.now();
    change({ pendingRequests: current.current.pendingRequests + 1 });
    try {
      const response = await transport.request(value);
      if (response.type === 'error') throw new EditorResponseError(response.payload.code, response.payload.message);
      return response;
    } finally {
      change({ pendingRequests: Math.max(0, current.current.pendingRequests - 1), lastRequestMs: performance.now() - started });
    }
  }, [change, transport]);
  const query = useCallback((payload: EditorQuery) => request({ category: 'query', payload }), [request]);

  const loadMaterial = useCallback(async (id: string, inspection: number) => {
    const sequence = ++materialSequence.current;
    try {
      const response = await query({ type: 'get_material', material_id: id });
      if (epoch.current !== inspection || sequence !== materialSequence.current || current.current.materialId !== id) return;
      if (response.type !== 'material') throw new Error('Unexpected material response');
      change({ material: response.payload });
    } catch (error) {
      if (epoch.current === inspection && sequence === materialSequence.current) change({ error: String(error) });
    }
  }, [change, query]);

  const selectMaterial = useCallback((id: string | null) => {
    if (current.current.materialId === id) return;
    ++materialSequence.current;
    ++materialEpoch.current;
    fieldRevisions.current = {};
    change({ materialId: id, material: null, materialDraft: {} });
    if (id) void loadMaterial(id, epoch.current);
  }, [change, loadMaterial]);

  const loadDetails = useCallback(async (target: InspectedObject, inspection: number, preferredSubmesh?: number) => {
    const sequence = ++detailsSequence.current;
    change({ detailsStatus: 'loading' });
    try {
      const response = await query(target.type === 'instance'
        ? { type: 'get_instance_details', instance_id: target.instance_id }
        : target.type === 'light' ? { type: 'get_light_details', light_id: target.light_id } : { type: 'get_environment' });
      if (epoch.current !== inspection || sequence !== detailsSequence.current) return;
      let details: InspectorDetails;
      if (response.type === 'instance_details') details = { type: 'instance', value: response.payload };
      else if (response.type === 'light_details') details = { type: 'light', value: response.payload };
      else if (response.type === 'environment') details = { type: 'environment', value: response.payload };
      else throw new Error('Unexpected object details response');
      observeVersion(details.value.scene_version);
      change({ details, detailsStatus: 'ready' });
      if (details.type === 'instance') {
        const bindings = details.value.materials;
        const binding = preferredSubmesh !== undefined ? bindings.find((item) => item.submesh_index === preferredSubmesh)
          : bindings.find((item) => item.material_id === current.current.materialId);
        const id = (binding ?? bindings[0])?.material_id ?? null;
        if (id !== current.current.materialId) selectMaterial(id);
        else if (id) await loadMaterial(id, inspection);
      }
    } catch (error) {
      if (epoch.current !== inspection || sequence !== detailsSequence.current) return;
      const stale = error instanceof EditorResponseError && error.code === 'stale_object';
      change({ ...(stale ? { details: null, material: null } : {}), detailsStatus: stale ? 'stale' : 'error', error: String(error) });
    }
  }, [change, loadMaterial, observeVersion, query, selectMaterial]);

  const inspectObject = useCallback((target: InspectedObject | null, submesh?: number) => {
    if (submesh === undefined && JSON.stringify(target) === JSON.stringify(current.current.inspectedObject)) return;
    const inspection = ++epoch.current;
    ++selectionSequence.current;
    ++detailsSequence.current;
    ++materialSequence.current;
    ++materialEpoch.current;
    fieldRevisions.current = {};
    change({ inspectedObject: target, details: null, detailsStatus: target ? 'loading' : 'idle',
      materialId: null, material: null, materialDraft: {}, lightDraft: {}, environmentDraft: {}, error: null });
    if (target) void loadDetails(target, inspection, submesh);
  }, [change, loadDetails]);

  const acceptSelection = useCallback((selection: SelectionDto | null) => {
    if (JSON.stringify(selection) === JSON.stringify(current.current.selection)) return;
    change({ selection });
    if (selection?.type === 'submesh') inspectObject({ type: 'instance', instance_id: selection.instance_id }, selection.submesh_index);
    else if (selection?.type === 'light') inspectObject({ type: 'light', light_id: selection.light_id });
    else inspectObject(null);
  }, [change, inspectObject]);

  const loadObjects = useCallback(async () => {
    const sequence = ++listSequence.current;
    for (let attempt = 0; attempt < 3; attempt++) {
      let offset = 0;
      let version: string | null = null;
      const objects: SceneObjectSummary[] = [];
      try {
        while (true) {
          const response = await query({ type: 'get_scene_objects', offset, limit: 128, expected_scene_version: version });
          if (sequence !== listSequence.current) return;
          if (response.type !== 'scene_objects') throw new Error('Unexpected scene objects response');
          version = response.payload.scene_version;
          objects.push(...response.payload.objects);
          if (response.payload.next_offset === null) {
            objectsVersion.current = version;
            change({ objects }); observeVersion(version); return;
          }
          offset = response.payload.next_offset;
        }
      } catch (error) {
        if (sequence !== listSequence.current) return;
        if (error instanceof EditorResponseError && error.code === 'conflict') continue;
        change({ error: String(error) }); return;
      }
    }
    change({ error: 'Scene changed repeatedly while loading objects' });
  }, [change, observeVersion, query]);

  const refresh = useCallback(async () => {
    const target = current.current.inspectedObject;
    await Promise.all([loadObjects(), target ? loadDetails(target, epoch.current) : Promise.resolve()]);
  }, [loadDetails, loadObjects]);

  const updateDraft = useCallback((patch: Partial<MaterialDto>) => {
    for (const key of Object.keys(patch)) fieldRevisions.current[key] = ++editRevision.current;
    change({ materialDraft: { ...current.current.materialDraft, ...patch } });
  }, [change]);
  const updateLightDraft = useCallback((patch: Partial<LightPatch>) => {
    for (const key of Object.keys(patch)) fieldRevisions.current[key] = ++editRevision.current;
    change({ lightDraft: { ...current.current.lightDraft, ...patch } });
  }, [change]);
  const updateEnvironmentDraft = useCallback((patch: Partial<EnvironmentPatch>) => {
    for (const key of Object.keys(patch)) fieldRevisions.current[key] = ++editRevision.current;
    change({ environmentDraft: { ...current.current.environmentDraft, ...patch } });
  }, [change]);

  // 同一页面的 mutation 按用户提交顺序发送；回包只确认对应字段的 revision。
  const commit = useCallback((command: EditorCommand) => {
    const inspection = epoch.current;
    const materialId = current.current.materialId;
    const materialInspection = materialEpoch.current;
    const submitted = Object.entries(command.patch).filter(([, value]) => value !== null && value !== undefined)
      .map(([key]) => key === 'texture_mappings' ? 'textures' : key);
    const revisions = { ...fieldRevisions.current };
    const work = commandTail.current.then(async () => {
      try {
        if (epoch.current === inspection) change({ error: null });
        const response = await request({ category: 'command', payload: command });
        if (epoch.current !== inspection || (command.type === 'update_material' && (current.current.materialId !== materialId || materialEpoch.current !== materialInspection))) return;
        const clearAcknowledged = <T extends object>(draft: T): T => {
          const next = { ...draft };
          for (const key of submitted) if (fieldRevisions.current[key] === revisions[key]) {
            delete next[key as keyof T]; delete fieldRevisions.current[key];
          }
          return next;
        };
        if (response.type === 'command_applied') {
          ++materialSequence.current;
          observeVersion(response.payload.scene_version);
          change({ material: response.payload.material, materialDraft: clearAcknowledged(current.current.materialDraft) });
        } else if (response.type === 'light_applied' || response.type === 'environment_applied') {
          ++detailsSequence.current;
          observeVersion(response.payload.scene_version);
          const newer = !current.current.details || BigInt(response.payload.scene_version) >= BigInt(current.current.details.value.scene_version);
          if (response.type === 'light_applied') change({
            ...(newer ? { details: { type: 'light', value: response.payload } as InspectorDetails } : {}),
            detailsStatus: 'ready', lightDraft: clearAcknowledged(current.current.lightDraft),
          });
          else change({
            ...(newer ? { details: { type: 'environment', value: response.payload } as InspectorDetails } : {}),
            detailsStatus: 'ready', environmentDraft: clearAcknowledged(current.current.environmentDraft),
          });
        } else throw new Error('Unexpected command response');
      } catch (error) {
        if (epoch.current === inspection && current.current.materialId === materialId && materialEpoch.current === materialInspection) change({ error: String(error) });
      }
    });
    commandTail.current = work;
    return work;
  }, [change, observeVersion, request]);

  const commitMaterial = useCallback((patch: MaterialPatch) => {
    const id = current.current.materialId;
    return id ? commit({ type: 'update_material', material_id: id, patch }) : Promise.resolve();
  }, [commit]);
  const commitLight = useCallback((patch: Partial<LightPatch>) => {
    const target = current.current.inspectedObject;
    return target?.type === 'light' ? commit({ type: 'update_light', light_id: target.light_id, patch: {
      position: null, radiance: null, direction: null, inner_angle_degrees: null, outer_angle_degrees: null,
      rotation_degrees: null, width: null, height: null, ...patch,
    } }) : Promise.resolve();
  }, [commit]);
  const commitEnvironment = useCallback((patch: Partial<EnvironmentPatch>) =>
    current.current.inspectedObject?.type === 'environment'
      ? commit({ type: 'update_environment', patch: { enabled: null, brightness: null, ...patch } }) : Promise.resolve(), [commit]);

  useEffect(() => {
    active.current = true;
    let disposed = false;
    let polling = false;
    const removeState = transport.onState((backendState) => change({ backendState }));
    const removeNotification = transport.onNotification((notification: EditorNotification) => {
      if (disposed) return;
      if (notification.type === 'selection_changed') {
        ++selectionSequence.current; acceptSelection(notification.payload);
      } else { observeVersion(notification.payload); void refresh(); }
    });
    const poll = async () => {
      if (polling || disposed) return;
      polling = true;
      const selectionRequest = ++selectionSequence.current;
      try {
        const [version, selection] = await Promise.all([query({ type: 'get_scene_version' }), query({ type: 'get_selection' })]);
        if (disposed) return;
        if (selection.type === 'selection' && selectionRequest === selectionSequence.current) acceptSelection(selection.payload);
        if (version.type === 'scene_version' && (BigInt(version.payload) > BigInt(objectsVersion.current)
          || (current.current.inspectedObject && (!current.current.details || BigInt(version.payload) > BigInt(current.current.details.value.scene_version))))) await refresh();
        else if (current.current.details?.type === 'environment' && current.current.details.value.load_state === 'loading') {
          // CPU asset 完成不一定推进 scene version；加载期间继续读取权威 record。
          await loadDetails({ type: 'environment' }, epoch.current);
        }
      } catch (error) { if (!disposed) change({ error: String(error) }); }
      finally { polling = false; }
    };
    void transport.connect().then(async () => { if (!disposed) { await poll(); await refresh(); } })
      .catch((error) => { if (!disposed) change({ error: String(error) }); });
    const timer = window.setInterval(() => { if (current.current.backendState === 'ready') void poll(); }, 1000);
    return () => {
      disposed = true; active.current = false; ++epoch.current; ++selectionSequence.current;
      window.clearInterval(timer); removeState(); removeNotification(); transport.close();
    };
  }, [acceptSelection, change, loadDetails, observeVersion, query, refresh, transport]);

  return { state, refresh, inspectObject, selectMaterial, updateDraft, commitMaterial,
    updateLightDraft, commitLight, updateEnvironmentDraft, commitEnvironment,
    materialDraft: state.material ? { ...state.material, ...state.materialDraft } : null,
  };
}
