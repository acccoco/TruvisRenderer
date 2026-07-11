import { MaterialInspector } from './components/material_inspector';
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
      <main className="editor-grid">
        <ScenePanel
          objects={state.objects}
          selection={state.selection}
          pageOffset={state.pageOffset}
          nextOffset={state.nextOffset}
          onPreviousPage={() => void previousPage()}
          onNextPage={() => void nextPage()}
        />
        <SelectionPanel selection={state.selection} material={state.draft} dirty={state.dirty} />
        <MaterialInspector
          material={state.draft}
          dirty={state.dirty}
          updateDraft={updateDraft}
          commitMaterial={commitMaterial}
        />
      </main>
      <StatusBar
        connected={state.connection === 'connected'}
        pendingRequests={state.pendingRequests}
        lastRequestMs={state.lastRequestMs}
        error={state.error}
      />
    </div>
  );
}
