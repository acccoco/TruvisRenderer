import type { LightDetailsDto } from '../protocol/generated';
import { TransformValues } from './transform_values';

/** 灯光只读投影；移动由原生 viewport gizmo 提交到 GameWorld。 */
export function LightInspector({ details }: { details: LightDetailsDto | null }) {
  return (
    <section className="panel instance-inspector" aria-labelledby="light-inspector-title">
      <div className="panel-heading"><h2 id="light-inspector-title">Light Inspector</h2></div>
      {details ? (
        <div className="instance-details">
          <section className="instance-detail-section">
            <h3>{details.kind === 'point' ? 'Point Light' : details.kind === 'spot' ? 'Spot Light' : 'Area Light'}</h3>
            <code title={details.light_id}>{details.light_id}</code>
          </section>
          <section className="instance-detail-section">
            <h3>World Position</h3>
            <dl className="transform-trs"><div className="transform-row">
              <dt>Location</dt><dd><TransformValues label="Location" values={details.position} /></dd>
            </div></dl>
          </section>
        </div>
      ) : <p className="instance-detail-note">Light details unavailable or loading.</p>}
    </section>
  );
}
