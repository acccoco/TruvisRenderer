import { EditorWorkspace } from './components/editor_workspace';
import { MaterialInspector } from './components/material_inspector';
import { RenderViewport } from './components/render_viewport';
import { ScenePanel } from './components/scene_panel';
import { SelectionPanel } from './components/selection_panel';
import { StatusBar } from './components/status_bar';
import { TopBar } from './components/top_bar';
import { useEditorSession } from './state/use_editor_session';

export function App() {
  const { state, refresh, nextPage, previousPage, updateDraft, commitMaterial } = useEditorSession();

  return (
    <div className="app-shell">
      <TopBar
        connection={state.connection}
        sceneVersion={state.sceneVersion}
        pendingRequests={state.pendingRequests}
        onRefresh={() => void refresh()}
      />
      <EditorWorkspace
        scenePanel={(
          <ScenePanel
            objects={state.objects}
            selection={state.selection}
            pageOffset={state.pageOffset}
            nextOffset={state.nextOffset}
            onPreviousPage={() => void previousPage()}
            onNextPage={() => void nextPage()}
          />
        )}
        viewport={<RenderViewport />}
        inspector={(
          <aside className="inspector-sidebar" aria-label="Selection and material inspector">
            <SelectionPanel selection={state.selection} material={state.draft} dirty={state.dirty} />
            <MaterialInspector
              material={state.draft}
              dirty={state.dirty}
              updateDraft={updateDraft}
              commitMaterial={commitMaterial}
            />
          </aside>
        )}
      />
      <StatusBar
        connected={state.connection === 'connected'}
        pendingRequests={state.pendingRequests}
        lastRequestMs={state.lastRequestMs}
        error={state.error}
      />
    </div>
  );
}
