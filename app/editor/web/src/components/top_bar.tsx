import type { ConnectionState } from '../transport/editor_transport';
import { EnvironmentIcon, RefreshIcon, TruvisMark } from './icons';

interface TopBarProps {
  connection: ConnectionState;
  sceneVersion: string;
  pendingRequests: number;
  desktopSkySupported: boolean;
  selectingSky: boolean;
  lastRequestedSkyFile: string | null;
  onRefresh(): void;
  onChooseHdri(): void;
}

export function TopBar({
  connection,
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
        <span className={`connection connection--${connection}`}>
          <span className="status-dot" />
          {connection === 'connected' ? 'Connected' : connection === 'connecting' ? 'Connecting' : 'Disconnected'}
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
        <button className="icon-button icon-button--labeled" type="button" onClick={onRefresh} disabled={connection !== 'connected'}>
          <RefreshIcon className={pendingRequests > 0 ? 'is-spinning' : undefined} />
          Refresh
        </button>
      </div>
    </header>
  );
}
