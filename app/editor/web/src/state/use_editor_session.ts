import { useCallback, useEffect, useReducer, useRef, useState } from 'react';

import type {
  EditorCapabilities,
  EditorNotification,
  EditorQuery,
  EditorRequest,
  EditorResponse,
  MaterialDto,
  MaterialPatch,
  SceneObjectSummary,
  SelectionDto,
} from '../protocol/generated';
import { createEditorTransport } from '../transport/create_editor_transport';
import type { ConnectionState, EditorTransport } from '../transport/editor_transport';

export interface EditorSessionState {
  connection: ConnectionState;
  capabilities: EditorCapabilities | null;
  sceneVersion: string;
  selection: SelectionDto | null;
  objects: SceneObjectSummary[];
  pageOffset: number;
  nextOffset: number | null;
  material: MaterialDto | null;
  draft: MaterialDto | null;
  dirty: boolean;
  pendingRequests: number;
  lastRequestMs: number | null;
  error: string | null;
}

type Action =
  | { type: 'connection'; value: ConnectionState }
  | { type: 'capabilities'; value: EditorCapabilities }
  | { type: 'sceneVersion'; value: string }
  | { type: 'selection'; value: SelectionDto | null }
  | { type: 'objects'; value: { objects: SceneObjectSummary[]; offset: number; nextOffset: number | null } }
  | { type: 'material'; value: MaterialDto | null }
  | { type: 'draft'; value: Partial<MaterialDto> }
  | { type: 'requestStart' }
  | { type: 'requestEnd'; elapsedMs: number }
  | { type: 'error'; value: string | null };

const initialState: EditorSessionState = {
  connection: 'disconnected',
  capabilities: null,
  sceneVersion: '—',
  selection: null,
  objects: [],
  pageOffset: 0,
  nextOffset: null,
  material: null,
  draft: null,
  dirty: false,
  pendingRequests: 0,
  lastRequestMs: null,
  error: null,
};

function reducer(state: EditorSessionState, action: Action): EditorSessionState {
  switch (action.type) {
    case 'connection':
      return { ...state, connection: action.value };
    case 'capabilities':
      return { ...state, capabilities: action.value };
    case 'sceneVersion':
      return { ...state, sceneVersion: action.value };
    case 'selection': {
      const keepsMaterial = state.selection?.material_id === action.value?.material_id;
      return {
        ...state,
        selection: action.value,
        material: keepsMaterial ? state.material : null,
        draft: keepsMaterial ? state.draft : null,
        dirty: keepsMaterial ? state.dirty : false,
      };
    }
    case 'objects':
      return {
        ...state,
        objects: action.value.objects,
        pageOffset: action.value.offset,
        nextOffset: action.value.nextOffset,
      };
    case 'material':
      return { ...state, material: action.value, draft: action.value, dirty: false };
    case 'draft':
      return state.draft ? { ...state, draft: { ...state.draft, ...action.value }, dirty: true } : state;
    case 'requestStart':
      return { ...state, pendingRequests: state.pendingRequests + 1, error: null };
    case 'requestEnd':
      return { ...state, pendingRequests: Math.max(0, state.pendingRequests - 1), lastRequestMs: action.elapsedMs };
    case 'error':
      return { ...state, error: action.value, pendingRequests: 0 };
  }
}

export interface EditorSession {
  state: EditorSessionState;
  refresh(): Promise<void>;
  nextPage(): Promise<void>;
  previousPage(): Promise<void>;
  updateDraft(patch: Partial<MaterialDto>): void;
  commitMaterial(patch: MaterialPatch): Promise<void>;
}

export function useEditorSession(): EditorSession {
  const [state, dispatch] = useReducer(reducer, initialState);
  const [transport] = useState<EditorTransport>(() => createEditorTransport());
  const sceneVersionRef = useRef(initialState.sceneVersion);

  useEffect(() => {
    sceneVersionRef.current = state.sceneVersion;
  }, [state.sceneVersion]);

  const request = useCallback(
    async (requestValue: EditorRequest): Promise<EditorResponse> => {
      const startedAt = performance.now();
      dispatch({ type: 'requestStart' });
      try {
        const response = await transport.request(requestValue);
        if (response.type === 'error') {
          throw new Error(response.payload.message);
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
      if (!selection) {
        dispatch({ type: 'material', value: null });
        return;
      }
      const response = await query({ type: 'get_material', material_id: selection.material_id });
      if (response.type === 'material') {
        dispatch({ type: 'material', value: response.payload });
      }
    },
    [query],
  );

  const loadPage = useCallback(
    async (offset: number, expectedSceneVersion: string | null) => {
      const response = await query({
        type: 'get_scene_objects',
        offset,
        limit: 128,
        expected_scene_version: expectedSceneVersion,
      });
      if (response.type === 'scene_objects') {
        dispatch({ type: 'sceneVersion', value: response.payload.scene_version });
        dispatch({
          type: 'objects',
          value: {
            objects: response.payload.objects,
            offset,
            nextOffset: response.payload.next_offset,
          },
        });
      }
    },
    [query],
  );

  const refresh = useCallback(async () => {
    try {
      const [capabilities, version, selection, objects] = await Promise.all([
        query({ type: 'get_capabilities' }),
        query({ type: 'get_scene_version' }),
        query({ type: 'get_selection' }),
        query({ type: 'get_scene_objects', offset: 0, limit: 128, expected_scene_version: null }),
      ]);
      if (capabilities.type === 'capabilities') {
        dispatch({ type: 'capabilities', value: capabilities.payload });
      }
      if (version.type === 'scene_version') {
        dispatch({ type: 'sceneVersion', value: version.payload });
      }
      if (objects.type === 'scene_objects') {
        dispatch({ type: 'sceneVersion', value: objects.payload.scene_version });
        dispatch({
          type: 'objects',
          value: { objects: objects.payload.objects, offset: 0, nextOffset: objects.payload.next_offset },
        });
      }
      if (selection.type === 'selection') {
        dispatch({ type: 'selection', value: selection.payload });
        await loadMaterial(selection.payload);
      }
    } catch (error) {
      dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
    }
  }, [loadMaterial, query]);

  useEffect(() => {
    let active = true;
    const removeConnectionListener = transport.onConnectionState((connection) => {
      if (active) {
        dispatch({ type: 'connection', value: connection });
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
        dispatch({ type: 'selection', value: notification.payload });
        void loadMaterial(notification.payload).catch((error) => {
          dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
        });
      }
    });

    void transport
      .connect()
      .then(() => refresh())
      .catch((error) => dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) }));

    return () => {
      active = false;
      removeConnectionListener();
      removeNotificationListener();
      transport.close();
    };
  }, [loadMaterial, refresh, transport]);

  useEffect(() => {
    if (state.connection !== 'connected') {
      return;
    }

    let active = true;
    let polling = false;
    const timer = window.setInterval(() => {
      if (polling) {
        return;
      }
      polling = true;
      void query({ type: 'get_scene_version' })
        .then((response) => {
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
  }, [query, refresh, state.connection]);

  const nextPage = useCallback(async () => {
    if (state.nextOffset === null) {
      return;
    }
    try {
      await loadPage(state.nextOffset, state.sceneVersion === '—' ? null : state.sceneVersion);
    } catch (error) {
      dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
    }
  }, [loadPage, state.nextOffset, state.sceneVersion]);

  const previousPage = useCallback(async () => {
    try {
      await loadPage(Math.max(0, state.pageOffset - 128), state.sceneVersion === '—' ? null : state.sceneVersion);
    } catch (error) {
      dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
    }
  }, [loadPage, state.pageOffset, state.sceneVersion]);

  const updateDraft = useCallback((patch: Partial<MaterialDto>) => {
    dispatch({ type: 'draft', value: patch });
  }, []);

  const commitMaterial = useCallback(
    async (patch: MaterialPatch) => {
      if (!state.draft) {
        return;
      }
      try {
        const response = await request({
          category: 'command',
          payload: { type: 'update_material', material_id: state.draft.id, patch },
        });
        if (response.type === 'command_applied') {
          dispatch({ type: 'sceneVersion', value: response.payload.scene_version });
          dispatch({ type: 'material', value: response.payload.material });
        }
      } catch (error) {
        dispatch({ type: 'error', value: error instanceof Error ? error.message : String(error) });
      }
    },
    [request, state.draft],
  );

  return { state, refresh, nextPage, previousPage, updateDraft, commitMaterial };
}
