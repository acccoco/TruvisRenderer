import type { InstanceDetailsDto } from '../protocol/generated';
import type { InstanceDetailsStatus } from '../state/use_editor_session';

interface InstanceInspectorProps {
  /** 当前 Web inspector focus 对应的 owned CPU scene 投影。 */
  details: InstanceDetailsDto | null;

  /** 详情查询状态；它只描述 Web 投影，不表示 render-side instance GPU ready。 */
  status: InstanceDetailsStatus;
}

/**
 * 当前 Web inspector focus 的只读 instance 信息面板。
 *
 * 面板不拥有 scene selection，也不发送 World mutation。完整 opaque ID 始终保留用于
 * 重名消歧，名称只负责主展示；material 顺序显式显示 submesh index，避免页面建立
 * 另一套 binding 关系。
 */
export function InstanceInspector({ details, status }: InstanceInspectorProps) {
  const statusLabel = status === 'loading' ? 'Refreshing…' : details ? 'World state' : 'No focus';

  return (
    <section className="panel instance-inspector" aria-labelledby="instance-inspector-title">
      <div className="panel-heading">
        <h2 id="instance-inspector-title">Instance Inspector</h2>
        <span className={status === 'loading' ? 'instance-state instance-state--loading' : 'instance-state'}>
          <span className="status-dot" />
          {statusLabel}
        </span>
      </div>

      {details ? (
        <div className="instance-details">
          <section className="instance-detail-section">
            <h3>{details.name}</h3>
            <code title={details.instance_id}>{details.instance_id}</code>
          </section>

          <section className="instance-detail-section">
            <h3>Transform</h3>
            <div className="transform-matrix" aria-label="Row-major world transform matrix">
              {details.transform.flatMap((row, rowIndex) =>
                row.map((value, columnIndex) => (
                  <output key={`${rowIndex}-${columnIndex}`}>{formatMatrixValue(value)}</output>
                )),
              )}
            </div>
          </section>

          <section className="instance-detail-section">
            <h3>Mesh</h3>
            <strong>{details.mesh.name}</strong>
            <code title={details.mesh.mesh_id}>{details.mesh.mesh_id}</code>
          </section>

          <section className="instance-detail-section">
            <h3>Material Bindings</h3>
            {details.materials.length > 0 ? (
              <ol className="material-binding-list">
                {details.materials.map((binding) => (
                  <li key={binding.submesh_index}>
                    <span className="binding-index">Submesh {binding.submesh_index}</span>
                    <strong>{binding.name}</strong>
                    <code title={binding.material_id}>{binding.material_id}</code>
                  </li>
                ))}
              </ol>
            ) : (
              <p className="instance-detail-note">No material bindings.</p>
            )}
          </section>
        </div>
      ) : (
        <InstanceInspectorEmptyState status={status} />
      )}
    </section>
  );
}

function InstanceInspectorEmptyState({ status }: { status: InstanceDetailsStatus }) {
  const content = status === 'loading'
    ? ['Loading instance…', 'Reading the current CPU World projection.']
    : status === 'stale'
      ? ['Instance no longer exists', 'Choose another instance from the scene list.']
      : status === 'error'
        ? ['Instance details unavailable', 'Review the status bar, then retry the selection.']
        : ['No instance inspected', 'Choose an instance from the scene list or pick a surface in the viewport.'];

  return (
    <div className="selection-empty">
      <h3>{content[0]}</h3>
      <p>{content[1]}</p>
    </div>
  );
}

function formatMatrixValue(value: number): string {
  const normalized = Math.abs(value) < 0.00005 ? 0 : value;
  return normalized.toFixed(4);
}
