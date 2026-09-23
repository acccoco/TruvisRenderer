import { useCallback, useEffect, useReducer, useRef, useState } from 'react';

import type {
  EditorErrorCode,
  EditorNotification,
  EditorQuery,
  EditorRequest,
  EditorResponse,
  InstanceDetailsDto,
  LightDetailsDto,
  MaterialDto,
  MaterialPatch,
  SceneObjectSummary,
  SelectionDto,
} from '../protocol/generated';
import { createEditorTransport } from '../transport/create_editor_transport';
import type { EditorBackendState, EditorTransport } from '../transport/editor_transport';

export type InstanceDetailsStatus = 'idle' | 'loading' | 'ready' | 'stale' | 'error';

export interface EditorSessionState {
  backendState: EditorBackendState;
  sceneVersion: string;
  selection: SelectionDto | null;
  objects: SceneObjectSummary[];
  inspectedInstanceId: string | null;
  instanceDetails: InstanceDetailsDto | null;
  inspectedLightId: string | null;
  lightDetails: LightDetailsDto | null;
  instanceDetailsStatus: InstanceDetailsStatus;
  material: MaterialDto | null;
  draft: MaterialDto | null;
  dirty: boolean;
  draftRevision: number;
  pendingRequests: number;
  lastRequestMs: number | null;
  error: string | null;
}

type Action =
  | { type: 'backendState'; value: EditorBackendState }
  | { type: 'sceneVersion'; value: string }
  | { type: 'selection'; value: SelectionDto | null }
  | { type: 'objects'; value: { objects: SceneObjectSummary[]; sceneVersion: string } }
  | { type: 'lightDetails'; value: LightDetailsDto | null; lightId: string }
  | { type: 'instanceDetailsStart'; instanceId: string }
  | { type: 'instanceDetailsReady'; value: InstanceDetailsDto }
  | { type: 'instanceDetailsStale'; instanceId: string }
  | { type: 'instanceDetailsFailed'; instanceId: string }
  | { type: 'material'; value: MaterialDto | null; acknowledgedDraftRevision?: number }
  | { type: 'draft'; value: Partial<MaterialDto>; revision: number }
  | { type: 'requestStart'; clearError: boolean }
  | { type: 'requestEnd'; elapsedMs: number }
  | { type: 'error'; value: string | null };

const initialState: EditorSessionState = {
  backendState: 'unavailable',
  sceneVersion: '—',
  selection: null,
  objects: [],
  inspectedInstanceId: null,
  instanceDetails: null,
  inspectedLightId: null,
  lightDetails: null,
  instanceDetailsStatus: 'idle',
  material: null,
  draft: null,
  dirty: false,
  draftRevision: 0,
  pendingRequests: 0,
  lastRequestMs: null,
  error: null,
};

function reducer(state: EditorSessionState, action: Action): EditorSessionState {
  switch (action.type) {
    case 'backendState':
      return { ...state, backendState: action.value };
    case 'sceneVersion':
      return { ...state, sceneVersion: action.value };
    case 'selection': {
      const keepsMaterial = state.selection?.type === 'submesh' && action.value?.type === 'submesh'
        && state.selection.material_id === action.value.material_id;
      const lightId = action.value?.type === 'light' ? action.value.light_id : null;
      const previousLightId = state.selection?.type === 'light' ? state.selection.light_id : null;
      const lightChanged = lightId !== previousLightId;
      return {
        ...state,
        selection: action.value,
        ...(lightChanged ? { inspectedLightId: lightId, lightDetails: null } : {}),
        ...(lightChanged && lightId ? { inspectedInstanceId: null, instanceDetails: null, instanceDetailsStatus: 'idle' as const } : {}),
        material: keepsMaterial ? state.material : null,
        draft: keepsMaterial ? state.draft : null,
        dirty: keepsMaterial ? state.dirty : false,
      };
    }
    case 'objects':
      return {
        ...state,
        objects: action.value.objects,
        sceneVersion: action.value.sceneVersion,
      };
    case 'lightDetails':
      return state.inspectedLightId === action.lightId ? { ...state, lightDetails: action.value } : state;
    case 'instanceDetailsStart': {
      const sameInstance = state.inspectedInstanceId === action.instanceId;
      return {
        ...state,
        inspectedInstanceId: action.instanceId,
        inspectedLightId: null,
        lightDetails: null,
        instanceDetails: sameInstance ? state.instanceDetails : null,
        instanceDetailsStatus: 'loading',
      };
    }
    case 'instanceDetailsReady':
      if (state.inspectedInstanceId !== action.value.instance_id) return state;
      return {
        ...state,
        inspectedInstanceId: action.value.instance_id,
        instanceDetails: action.value,
        instanceDetailsStatus: 'ready',
      };
    case 'instanceDetailsStale':
      return state.inspectedInstanceId === action.instanceId
        ? { ...state, instanceDetails: null, instanceDetailsStatus: 'stale' }
        : state;
    case 'instanceDetailsFailed':
      return state.inspectedInstanceId === action.instanceId
        ? { ...state, instanceDetails: null, instanceDetailsStatus: 'error' }
        : state;
    case 'material': {
      if (action.value && (state.selection?.type !== 'submesh' || action.value.id !== state.selection.material_id)) return state;
      // 查询和较早提交的回包不能覆盖同一材质的后续输入；只有对应草稿的确认才能清除 dirty。
      const keepDraft = state.dirty && state.draft?.id === action.value?.id
        && action.acknowledgedDraftRevision !== state.draftRevision;
      return { ...state, material: action.value, draft: keepDraft ? state.draft : action.value, dirty: keepDraft };
    }
    case 'draft':
      return state.draft ? { ...state, draft: { ...state.draft, ...action.value }, dirty: true, draftRevision: action.revision } : state;
    case 'requestStart':
      return { ...state, pendingRequests: state.pendingRequests + 1, error: action.clearError ? null : state.error };
    case 'requestEnd':
      return { ...state, pendingRequests: Math.max(0, state.pendingRequests - 1), lastRequestMs: action.elapsedMs };
    case 'error':
      return { ...state, error: action.value, pendingRequests: 0 };
  }
}

class EditorResponseError extends Error {
  readonly code: EditorErrorCode;

  constructor(code: EditorErrorCode, message: string) {
    super(message);
    this.name = 'EditorResponseError';
    this.code = code;
  }
}

export interface EditorSession {
  state: EditorSessionState;
  refresh(): Promise<void>;
  inspectInstance(instanceId: string): Promise<void>;
  updateDraft(patch: Partial<MaterialDto>): void;
  commitMaterial(patch: MaterialPatch): Promise<void>;
}

export function useEditorSession(): EditorSession {
  const [state, dispatch] = useReducer(reducer, initialState);
  const [transport] = useState<EditorTransport>(() => createEditorTransport());
  const sceneVersionRef = useRef(initialState.sceneVersion);
  const inspectedInstanceIdRef = useRef<string | null>(null);
  const instanceDetailsRequestSequenceRef = useRef(0);
  const materialRequestSequenceRef = useRef(0);
  const lightDetailsRequestSequenceRef = useRef(0);
  const inspectedLightIdRef = useRef<string | null>(null);
  const selectionRef = useRef<SelectionDto | null>(null);
  const selectionEpochRef = useRef(0);
  const materialCommandSequenceRef = useRef(0);
  const draftRevisionRef = useRef(0);

  useEffect(() => {
    sceneVersionRef.current = state.sceneVersion;
  }, [state.sceneVersion]);

  useEffect(() => {
    inspectedInstanceIdRef.current = state.inspectedInstanceId;
    inspectedLightIdRef.current = state.inspectedLightId;
  }, [state.inspectedInstanceId, state.inspectedLightId]);

  const request = useCallback(
    async (requestValue: EditorRequest): Promise<EditorResponse> => {
      const startedAt = performance.now();
      // 后台轮询不能擦除材质校验错误；用户下一次提交时再清除旧错误。
      dispatch({ type: 'requestStart', clearError: requestValue.category === 'command' });
      try {
        const response = await transport.request(requestValue);
        if (response.type === 'error') {
          throw new EditorResponseError(response.payload.code, response.payload.message);
        }
        return response;
      } finally {
        dispatch({ type: 'requestEnd', elapsedMs: performance.now() - startedAt });
      }
    },
    [transport],
  );

  const query = useCallback(
    (payload: EditorQuery) => request({ category: 'query', payload }),
    [request],
  );

  const loadMaterial = useCallback(
    async (selection: SelectionDto | null) => {
      const sequence = ++materialRequestSequenceRef.current;
      if (selection?.type !== 'submesh') {
        dispatch({ type: 'material', value: null });
        return;
      }
      const response = await query({ type: 'get_material', material_id: selection.material_id });
      if (response.type === 'material' && sequence === materialRequestSequenceRef.current) {
        dispatch({ type: 'material', value: response.payload });
      }
    },
    [query],
  );

  /**
   * 查询 Web 当前聚焦 instance 的 owned 详情投影。
   *
   * sequence 只解决页面内快速切换产生的响应乱序，不成为 scene identity 或缓存版本；
   * scene 的权威失效判断仍来自 `scene_version` 和 App 返回的 `stale_object`。
   */
  const loadInstanceDetails = useCallback(
    async (instanceId: string) => {
      const requestSequence = ++instanceDetailsRequestSequenceRef.current;
      inspectedInstanceIdRef.current = instanceId;
      inspectedLightIdRef.current = null;
      ++lightDetailsRequestSequenceRef.current;
      dispatch({ type: 'instanceDetailsStart', instanceId });
      try {
        const response = await query({ type: 'get_instance_details', instance_id: instanceId });
        if (requestSequence !== instanceDetailsRequestSequenceRef.current) {
          return;
        }
        if (response.type !== 'instance_details') {
          throw new Error('Editor returned an unexpected instance details response');
        }
        dispatch({ type: 'instanceDetailsReady', value: response.payload });
      } catch (error) {
        if (requestSequence !== instanceDetailsRequestSequenceRef.current) {
          return;
        }
        if (error instanceof EditorResponseError && error.code === 'stale_object') {
          dispatch({ type: 'instanceDetailsStale', instanceId });
          return;
        }
        dispatch({ type: 'instanceDetailsFailed', instanceId });
        dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
      }
    },
    [query],
  );

  const loadLightDetails = useCallback(async (lightId: string) => {
    const sequence = ++lightDetailsRequestSequenceRef.current;
    try {
      const response = await query({ type: 'get_light_details', light_id: lightId });
      if (sequence !== lightDetailsRequestSequenceRef.current) return;
      if (response.type !== 'light_details') throw new Error('Unexpected light details response');
      dispatch({ type: 'lightDetails', lightId, value: response.payload });
    } catch (error) {
      if (sequence !== lightDetailsRequestSequenceRef.current) return;
      dispatch({ type: 'lightDetails', lightId, value: null });
      dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
    }
  }, [query]);

  // 通知和主动查询共用入口；改变身份时先使在途详情请求失效。
  const acceptSelection = useCallback((selection: SelectionDto | null) => {
    const changed = JSON.stringify(selectionRef.current) !== JSON.stringify(selection);
    if (changed) {
      ++selectionEpochRef.current;
      ++materialRequestSequenceRef.current;
      ++materialCommandSequenceRef.current;
      ++lightDetailsRequestSequenceRef.current;
      if (selection?.type === 'light') {
        ++instanceDetailsRequestSequenceRef.current;
        inspectedInstanceIdRef.current = null;
        inspectedLightIdRef.current = selection.light_id;
      } else {
        inspectedLightIdRef.current = null;
      }
    }
    selectionRef.current = selection;
    dispatch({ type: 'selection', value: selection });
    return changed;
  }, []);

  const loadAllSceneObjects = useCallback(async () => {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      let offset = 0;
      let expectedSceneVersion: string | null = null;
      const objects: SceneObjectSummary[] = [];

      try {
        while (true) {
          const response = await query({
            type: 'get_scene_objects',
            offset,
            limit: 128,
            expected_scene_version: expectedSceneVersion,
          });
          if (response.type !== 'scene_objects') {
            throw new Error('Editor returned an unexpected scene objects response');
          }

          expectedSceneVersion = response.payload.scene_version;
          objects.push(...response.payload.objects);
          if (response.payload.next_offset === null) {
            return { objects, sceneVersion: response.payload.scene_version };
          }
          offset = response.payload.next_offset;
        }
      } catch (error) {
        if (error instanceof EditorResponseError && error.code === 'conflict') {
          continue;
        }
        throw error;
      }
    }

    throw new Error('Scene changed repeatedly while loading objects');
  }, [query]);

  const refreshProjection = useCallback(async () => {
    const epoch = selectionEpochRef.current;
    try {
      const [selection, objects] = await Promise.all([
        query({ type: 'get_selection' }),
        loadAllSceneObjects(),
      ]);
      dispatch({ type: 'objects', value: objects });
      if (selection.type === 'selection' && epoch === selectionEpochRef.current) {
        acceptSelection(selection.payload);
        await loadMaterial(selection.payload);
      }
    } catch (error) {
      dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
    }
  }, [acceptSelection, loadAllSceneObjects, loadMaterial, query]);

  const refresh = useCallback(async () => {
    const inspectedInstanceId = inspectedInstanceIdRef.current;
    const inspectedLightId = inspectedLightIdRef.current;
    await Promise.all([
      refreshProjection(),
      inspectedInstanceId ? loadInstanceDetails(inspectedInstanceId) : Promise.resolve(),
      inspectedLightId ? loadLightDetails(inspectedLightId) : Promise.resolve(),
    ]);
  }, [loadInstanceDetails, loadLightDetails, refreshProjection]);

  const selectedInstanceId = state.selection?.type === 'submesh' ? state.selection.instance_id : null;
  const selectedLightId = state.selection?.type === 'light' ? state.selection.light_id : null;
  useEffect(() => {
    if (selectedLightId) void loadLightDetails(selectedLightId);
  }, [loadLightDetails, selectedLightId]);
  const previousSelectedInstanceIdRef = useRef<string | null>(null);

  useEffect(() => {
    if (selectedInstanceId && selectedInstanceId !== previousSelectedInstanceIdRef.current) {
      void loadInstanceDetails(selectedInstanceId);
    }
    previousSelectedInstanceIdRef.current = selectedInstanceId;
  }, [loadInstanceDetails, selectedInstanceId]);

  useEffect(() => {
    let active = true;
    const removeStateListener = transport.onState((backendState) => {
      if (active) {
        dispatch({ type: 'backendState', value: backendState });
      }
    });
    const removeNotificationListener = transport.onNotification((notification: EditorNotification) => {
      if (!active) {
        return;
      }
      if (notification.type === 'scene_version_changed') {
        dispatch({ type: 'sceneVersion', value: notification.payload });
        // 通知只携带失效信号；具体需要重新获取哪些场景投影由 Web 决定。
        void refresh();
      } else {
        acceptSelection(notification.payload);
        void loadMaterial(notification.payload).catch((error) => {
          dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
        });
      }
    });

    void transport
      .connect()
      .then(() => (active ? refreshProjection() : undefined))
      .catch((error) => {
        if (active) {
          dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
        }
      });

    return () => {
      active = false;
      removeStateListener();
      removeNotificationListener();
      transport.close();
    };
  }, [acceptSelection, loadMaterial, refresh, refreshProjection, transport]);

  useEffect(() => {
    if (state.backendState !== 'ready') {
      return;
    }

    let active = true;
    let polling = false;
    const timer = window.setInterval(() => {
      if (polling) {
        return;
      }
      polling = true;
      const epoch = selectionEpochRef.current;
      void Promise.all([query({ type: 'get_scene_version' }), query({ type: 'get_selection' })])
        .then(([response, selection]) => {
          if (active && epoch === selectionEpochRef.current && selection.type === 'selection') {
            if (acceptSelection(selection.payload)) void loadMaterial(selection.payload).catch((error) => dispatch({ type: 'error', value: String(error) }));
          }
          if (active && response.type === 'scene_version' && response.payload !== sceneVersionRef.current) {
            return refresh();
          }
        })
        .catch((error) => {
          if (active) {
            dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
          }
        })
        .finally(() => {
          polling = false;
        });
    }, 1_000);

    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [acceptSelection, loadMaterial, query, refresh, state.backendState]);

  const inspectInstance = useCallback(
    async (instanceId: string) => {
      await loadInstanceDetails(instanceId);
    },
    [loadInstanceDetails],
  );

  const updateDraft = useCallback((patch: Partial<MaterialDto>) => {
    dispatch({ type: 'draft', value: patch, revision: ++draftRevisionRef.current });
  }, []);

  const commitMaterial = useCallback(
    async (patch: MaterialPatch) => {
      if (!state.draft) {
        return;
      }
      ++materialRequestSequenceRef.current;
      const sequence = ++materialCommandSequenceRef.current;
      const draftRevision = draftRevisionRef.current;
      try {
        const response = await request({
          category: 'command',
          payload: { type: 'update_material', material_id: state.draft.id, patch },
        });
        if (response.type === 'command_applied') {
          dispatch({ type: 'sceneVersion', value: response.payload.scene_version });
          // 场景通知可能先发起查询；命令确认仍可确认对应草稿，但不能覆盖后续材质选择或输入。
          if (sequence === materialCommandSequenceRef.current) {
            ++materialRequestSequenceRef.current;
            dispatch({ type: 'material', value: response.payload.material, acknowledgedDraftRevision: draftRevision });
          }
        }
      } catch (error) {
        dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
      }
    },
    [request, state.draft],
  );

  return { state, refresh, inspectInstance, updateDraft, commitMaterial };
}
