import type { EditorBackendState } from '../transport/editor_transport';
import { EnvironmentIcon, RefreshIcon, TruvisMark } from './icons';

interface TopBarProps {
  backendState: EditorBackendState;
  sceneVersion: string;
  pendingRequests: number;
  desktopSkySupported: boolean;
  selectingSky: boolean;
  lastRequestedSkyFile: string | null;
  onRefresh(): void;
  onChooseHdri(): void;
}

export function TopBar({
  backendState,
  sceneVersion,
  pendingRequests,
  desktopSkySupported,
  selectingSky,
  lastRequestedSkyFile,
  onRefresh,
  onChooseHdri,
}: TopBarProps) {
  const chooseHdriTitle = desktopSkySupported
    ? 'Choose a Radiance HDR or OpenEXR environment'
    : 'Available in the Tauri desktop app';

  return (
    <header className="top-bar">
      <div className="brand">
        <TruvisMark />
        <h1>Truvis Editor</h1>
      </div>
      <div className="top-bar__status">
        <span className={`backend-state backend-state--${backendState}`}>
          <span className="status-dot" />
          {backendState === 'ready' ? 'Ready' : backendState === 'starting' ? 'Starting' : 'Unavailable'}
        </span>
        <span className="scene-version">
          Scene version <strong>{sceneVersion}</strong>
        </span>
        {lastRequestedSkyFile ? (
          <span className="sky-request" title={`HDRI requested: ${lastRequestedSkyFile}`}>
            HDRI requested <strong>{lastRequestedSkyFile}</strong>
          </span>
        ) : null}
        <button
          className="icon-button icon-button--labeled"
          type="button"
          onClick={onChooseHdri}
          disabled={!desktopSkySupported || selectingSky}
          title={chooseHdriTitle}
        >
          <EnvironmentIcon />
          {selectingSky ? 'Choosing…' : 'Choose HDRI'}
        </button>
        <button className="icon-button icon-button--labeled" type="button" onClick={onRefresh} disabled={backendState !== 'ready'}>
          <RefreshIcon className={pendingRequests > 0 ? 'is-spinning' : undefined} />
          Refresh
        </button>
      </div>
    </header>
  );
}
