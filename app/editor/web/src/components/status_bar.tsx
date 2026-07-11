interface StatusBarProps {
  connected: boolean;
  pendingRequests: number;
  lastRequestMs: number | null;
  error: string | null;
}

export function StatusBar({ connected, pendingRequests, lastRequestMs, error }: StatusBarProps) {
  return (
    <footer className="status-bar">
      <span className={connected ? 'status-bar__ready' : 'status-bar__offline'}>
        <span className="status-dot" />
        {connected ? 'Ready' : 'Offline'}
      </span>
      <span>{pendingRequests > 0 ? `${pendingRequests} request${pendingRequests > 1 ? 's' : ''} pending` : 'No pending requests'}</span>
      <span>{lastRequestMs === null ? 'No completed request' : `Last request ${Math.round(lastRequestMs)} ms`}</span>
      <span className={error ? 'status-bar__error' : 'status-bar__ok'}>{error ?? 'No errors'}</span>
    </footer>
  );
}
