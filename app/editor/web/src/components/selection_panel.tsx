import type { CSSProperties } from 'react';

import type { MaterialDto, SelectionDto } from '../protocol/generated';

interface SelectionPanelProps {
  selection: SelectionDto | null;
  material: MaterialDto | null;
  dirty: boolean;
}

function shortId(id: string | undefined): string {
  return id?.split(':')[1]?.slice(-8) ?? '—';
}

export function SelectionPanel({ selection, material, dirty }: SelectionPanelProps) {
  const rgb = material
    ? material.base_color.slice(0, 3).map((channel) => Math.round(Math.min(1, channel) * 255)).join(', ')
    : '74, 84, 90';
  const previewStyle = {
    '--material-rgb': rgb,
    '--material-metallic': material?.metallic ?? 0,
    '--material-roughness': material?.roughness ?? 0.5,
  } as CSSProperties;

  return (
    <section className="panel selection-panel" aria-labelledby="selection-title">
      <div className="panel-heading">
        <h2 id="selection-title">Selection</h2>
      </div>
      {selection ? (
        <>
          <dl className="selection-summary">
            <div>
              <dt>Object</dt>
              <dd>{shortId(selection.instance_id)}</dd>
            </div>
            <div>
              <dt>Submesh</dt>
              <dd>{selection.submesh_index}</dd>
            </div>
          </dl>
          <div className="preview-heading">Material Preview</div>
          <div className="material-preview" style={previewStyle}>
            <div className="material-sphere" />
            <div className="sphere-shadow" />
          </div>
          <dl className="material-summary">
            <div>
              <dt>Material</dt>
              <dd>{material?.name ?? 'Loading…'}</dd>
            </div>
            <div>
              <dt>Material ID</dt>
              <dd>{shortId(selection.material_id)}</dd>
            </div>
            <div>
              <dt>Class</dt>
              <dd>{material?.class.kind ?? '—'}</dd>
            </div>
            <div>
              <dt>Coverage</dt>
              <dd>{material?.coverage.kind ?? '—'}</dd>
            </div>
          </dl>
          <p className={`draft-note${dirty ? ' draft-note--dirty' : ''}`}>
            <span className="status-dot" />
            {dirty ? 'Local draft — release the mouse to commit.' : 'Material matches the current World state.'}
          </p>
        </>
      ) : (
        <div className="selection-empty">
          <div className="selection-empty__mark" />
          <h3>No material selected</h3>
          <p>Pick a surface in the Truvis render window to inspect its material.</p>
        </div>
      )}
    </section>
  );
}
