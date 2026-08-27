interface StatusBarProps {
  ready: boolean;
  pendingRequests: number;
  selectingSky: boolean;
  lastRequestMs: number | null;
  error: string | null;
}

export function StatusBar({ ready, pendingRequests, selectingSky, lastRequestMs, error }: StatusBarProps) {
  return (
    <footer className="status-bar">
      <span className={ready ? 'status-bar__ready' : 'status-bar__offline'}>
        <span className="status-dot" />
        {ready ? 'Renderer ready' : 'Renderer unavailable'}
      </span>
      <span>
        {selectingSky
          ? 'Choosing HDRI…'
          : pendingRequests > 0
            ? `${pendingRequests} request${pendingRequests > 1 ? 's' : ''} pending`
            : 'No pending requests'}
      </span>
      <span>{lastRequestMs === null ? 'No completed request' : `Last request ${Math.round(lastRequestMs)} ms`}</span>
      <span className={error ? 'status-bar__error' : 'status-bar__ok'}>{error ?? 'No errors'}</span>
    </footer>
  );
}
