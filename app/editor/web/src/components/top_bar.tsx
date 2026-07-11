import type { ConnectionState } from '../transport/editor_transport';
import { RefreshIcon, TruvisMark } from './icons';

interface TopBarProps {
  connection: ConnectionState;
  sceneVersion: string;
  pendingRequests: number;
  onRefresh(): void;
}

export function TopBar({ connection, sceneVersion, pendingRequests, onRefresh }: TopBarProps) {
  return (
    <header className="top-bar">
      <div className="brand">
        <TruvisMark />
        <h1>Truvis Material Editor</h1>
      </div>
      <div className="top-bar__status">
        <span className={`connection connection--${connection}`}>
          <span className="status-dot" />
          {connection === 'connected' ? 'Connected' : connection === 'connecting' ? 'Connecting' : 'Disconnected'}
        </span>
        <span className="scene-version">
          Scene version <strong>{sceneVersion}</strong>
        </span>
        <button className="icon-button icon-button--labeled" type="button" onClick={onRefresh} disabled={connection !== 'connected'}>
          <RefreshIcon className={pendingRequests > 0 ? 'is-spinning' : undefined} />
          Refresh
        </button>
      </div>
    </header>
  );
}
