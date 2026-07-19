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
  return (
    <section className="panel selection-panel" aria-labelledby="selection-title">
      <div className="panel-heading">
        <h2 id="selection-title">Current Selection</h2>
        <span className={dirty ? 'selection-state selection-state--dirty' : 'selection-state'}>
          <span className="status-dot" />
          {dirty ? 'Draft' : 'World state'}
        </span>
      </div>
      {selection ? (
        <dl className="selection-summary">
          <div>
            <dt>Object</dt>
            <dd title={selection.instance_id}>{shortId(selection.instance_id)}</dd>
          </div>
          <div>
            <dt>Submesh</dt>
            <dd>{selection.submesh_index}</dd>
          </div>
          <div>
            <dt>Material</dt>
            <dd title={selection.material_id}>{material?.name ?? shortId(selection.material_id)}</dd>
          </div>
          <div>
            <dt>Class</dt>
            <dd>{material?.class.kind ?? '—'}</dd>
          </div>
        </dl>
      ) : (
        <div className="selection-empty">
          <h3>No material selected</h3>
          <p>Pick a surface in the render viewport to inspect its material.</p>
        </div>
      )}
    </section>
  );
}
