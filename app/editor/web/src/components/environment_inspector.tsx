import type { EnvironmentDetailsDto, EnvironmentPatch } from '../protocol/generated';
import type { ObjectDetailsStatus } from '../state/use_editor_session';
import type { DesktopSkyAction } from '../state/use_desktop_sky_action';
import { NumberField } from './parameter_fields';

export function EnvironmentInspector({ details, status, draft, desktopSky, onDraft, onCommit }: {
  details: EnvironmentDetailsDto | null; status: ObjectDetailsStatus; draft: Partial<EnvironmentPatch>; desktopSky: DesktopSkyAction;
  onDraft(patch: Partial<EnvironmentPatch>): void; onCommit(patch: Partial<EnvironmentPatch>): Promise<void>;
}) {
  return <section className="panel inspector-panel" aria-labelledby="environment-title">
    <div className="panel-heading"><h2 id="environment-title">Environment / HDRI</h2>
      <span className={Object.keys(draft).length ? 'draft-state draft-state--dirty' : 'draft-state'}>{Object.keys(draft).length ? 'Local draft' : 'World state'}</span></div>
    {details ? <div className="inspector-fields">
      <label className="field field--row"><span>Enabled</span><input type="checkbox" checked={draft.enabled ?? details.enabled}
        onChange={(event) => { const enabled = event.target.checked; onDraft({ enabled }); void onCommit({ enabled }); }} /></label>
      <NumberField label="Brightness" value={draft.brightness ?? details.brightness} min={0}
        onDraft={(brightness) => onDraft({ brightness })} onCommit={(brightness) => void onCommit({ brightness })} />
      <div className="instance-detail-section"><h3>HDRI texture</h3><p className="texture-file" title={details.file_name ?? ''}>{details.file_name ?? 'Not set'}</p>
        <p>CPU load: {details.load_state}</p>
        <button type="button" disabled={!desktopSky.state.supported || desktopSky.state.selecting} onClick={() => void desktopSky.chooseHdri()}>
          {desktopSky.state.selecting ? 'Choosing…' : 'Choose HDRI'}</button>
        <p className="instance-detail-note">{desktopSky.state.supported ? 'CPU ready does not confirm GPU upload or rendering completion.' : 'File selection is available in the desktop editor.'}</p>
      </div>
    </div> : <p className="instance-detail-note">{status === 'loading' ? 'Loading environment…' : 'Environment unavailable. Refresh to retry.'}</p>}
  </section>;
}
