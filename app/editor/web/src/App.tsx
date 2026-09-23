import { EditorWorkspace } from './components/editor_workspace';
import { EnvironmentInspector } from './components/environment_inspector';
import { LightInspector } from './components/light_inspector';
import { InstanceInspector } from './components/instance_inspector';
import { MaterialInspector } from './components/material_inspector';
import { RenderViewport } from './components/render_viewport';
import { ScenePanel } from './components/scene_panel';
import { StatusBar } from './components/status_bar';
import { TopBar } from './components/top_bar';
import { useDesktopSkyAction } from './state/use_desktop_sky_action';
import { useEditorSession } from './state/use_editor_session';

export function App() {
  const session = useEditorSession();
  const { state, refresh, inspectObject, updateDraft, commitMaterial } = session;
  const desktopSky = useDesktopSkyAction();

  return (
    <div className="app-shell">
      <TopBar
        backendState={state.backendState}
        sceneVersion={state.sceneVersion}
        pendingRequests={state.pendingRequests}
        desktopSkySupported={desktopSky.state.supported}
        selectingSky={desktopSky.state.selecting}
        lastRequestedSkyFile={desktopSky.state.lastRequestedFile}
        onRefresh={() => void refresh()}
        onChooseHdri={() => void desktopSky.chooseHdri()}
      />
      <EditorWorkspace
        scenePanel={(
          <ScenePanel
            objects={state.objects}
            inspectedObject={state.inspectedObject}
            onInspectObject={inspectObject}
          />
        )}
        viewport={<RenderViewport />}
        inspector={(
          <aside className="inspector-sidebar" aria-label="Scene selection inspector">
            {state.inspectedObject?.type === 'light' ? <LightInspector key={state.inspectedObject.light_id}
              details={state.details?.type === 'light' ? state.details.value : null} status={state.detailsStatus}
              draft={state.lightDraft} onDraft={session.updateLightDraft} onCommit={session.commitLight} />
              : state.inspectedObject?.type === 'environment' ? <EnvironmentInspector
                details={state.details?.type === 'environment' ? state.details.value : null} status={state.detailsStatus}
                draft={state.environmentDraft} onDraft={session.updateEnvironmentDraft} onCommit={session.commitEnvironment} desktopSky={desktopSky} />
              : <>
                <InstanceInspector details={state.details?.type === 'instance' ? state.details.value : null} status={state.detailsStatus}
                  materialId={state.materialId} onSelectMaterial={session.selectMaterial} />
                {state.inspectedObject?.type === 'instance' && <MaterialInspector key={state.materialId}
                  material={session.materialDraft} dirty={Object.keys(state.materialDraft).length > 0}
                  updateDraft={updateDraft} commitMaterial={commitMaterial} />}
              </>}
          </aside>
        )}
      />
      <StatusBar
        ready={state.backendState === 'ready'}
        pendingRequests={state.pendingRequests}
        selectingSky={desktopSky.state.selecting}
        lastRequestMs={state.lastRequestMs}
        error={desktopSky.state.error ?? state.error}
      />
    </div>
  );
}
