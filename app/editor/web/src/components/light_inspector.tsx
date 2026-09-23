import type { LightDetailsDto, LightPatch } from '../protocol/generated';
import type { ObjectDetailsStatus } from '../state/use_editor_session';
import { NumberField, VectorField } from './parameter_fields';

export function LightInspector({ details, status, draft, onDraft, onCommit }: {
  details: LightDetailsDto | null; status: ObjectDetailsStatus; draft: Partial<LightPatch>;
  onDraft(patch: Partial<LightPatch>): void; onCommit(patch: Partial<LightPatch>): Promise<void>;
}) {
  const parameters = details?.parameters;
  return <section className="panel inspector-panel" aria-labelledby="light-inspector-title">
    <div className="panel-heading"><h2 id="light-inspector-title">Light Inspector</h2>
      <span className={Object.keys(draft).length ? 'draft-state draft-state--dirty' : 'draft-state'}>
        {Object.keys(draft).length ? 'Local draft' : status === 'loading' ? 'Refreshing…' : 'World state'}</span></div>
    {details && parameters ? <div className="inspector-fields">
      <div className="instance-detail-section"><h3>{parameters.kind} light</h3><code>{details.light_id}</code></div>
      <VectorField label="Position" value={draft.position ?? details.position} onDraft={(position) => onDraft({ position })} onCommit={(position) => void onCommit({ position })} />
      <VectorField label="Emission (linear RGB)" axes={['R', 'G', 'B']} min={0} value={draft.radiance ?? details.radiance} onDraft={(radiance) => onDraft({ radiance })} onCommit={(radiance) => void onCommit({ radiance })} />
      {parameters.kind === 'spot' && <>
        <VectorField label="Direction" value={draft.direction ?? parameters.direction} onDraft={(direction) => onDraft({ direction })} onCommit={(direction) => void onCommit({ direction })} />
        <NumberField label="Inner angle (°)" min={0} max={180} value={draft.inner_angle_degrees ?? parameters.inner_angle_degrees}
          onDraft={(inner_angle_degrees) => onDraft({ inner_angle_degrees })} onCommit={(inner_angle_degrees) => void onCommit({ inner_angle_degrees })} />
        <NumberField label="Outer angle (°)" min={0} max={180} value={draft.outer_angle_degrees ?? parameters.outer_angle_degrees}
          onDraft={(outer_angle_degrees) => onDraft({ outer_angle_degrees })} onCommit={(outer_angle_degrees) => void onCommit({ outer_angle_degrees })} />
      </>}
      {parameters.kind === 'area' && (parameters.shape ? <>
        <VectorField label="Rotation XYZ (°)" value={draft.rotation_degrees ?? parameters.shape.rotation_degrees}
          onDraft={(rotation_degrees) => onDraft({ rotation_degrees })} onCommit={(rotation_degrees) => void onCommit({ rotation_degrees })} />
        <NumberField label="Width" min={Number.MIN_VALUE} value={draft.width ?? parameters.shape.width} onDraft={(width) => onDraft({ width })} onCommit={(width) => void onCommit({ width })} />
        <NumberField label="Height" min={Number.MIN_VALUE} value={draft.height ?? parameters.shape.height} onDraft={(height) => onDraft({ height })} onCommit={(height) => void onCommit({ height })} />
      </> : <p className="instance-detail-note">Shape cannot be represented as rotation and dimensions. Position and radiance remain editable.</p>)}
    </div> : <p className="instance-detail-note">{status === 'loading' ? 'Loading light…' : 'Light details unavailable. Refresh to retry.'}</p>}
  </section>;
}
