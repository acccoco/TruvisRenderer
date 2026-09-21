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
  const transformRows = details?.transform ? [
    { label: 'Location', values: details.transform.location, unit: '' },
    { label: 'Rotation', values: details.transform.rotation_degrees, unit: '°', hint: 'XYZ intrinsic' },
    { label: 'Scale', values: details.transform.scale, unit: '' },
  ] : [];

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
            <h3>World Transform</h3>
            {details.transform ? (
              <dl className="transform-trs">
                {transformRows.map(({ label, values, unit, hint }) => (
                  <div className="transform-row" key={label}>
                    <dt>{label}{hint && <small>{hint}</small>}</dt>
                    <dd>
                      {values.map((value, index) => {
                        const axis = ['X', 'Y', 'Z'][index];
                        const formatted = value.toFixed(4);
                        const text = `${formatted === '-0.0000' ? '0.0000' : formatted}${unit}`;
                        return (
                          <div className="transform-axis" key={axis}>
                            <span aria-hidden="true">{axis}</span>
                            <output aria-label={`${label} ${axis}`} title={text}>{text}</output>
                          </div>
                        );
                      })}
                    </dd>
                  </div>
                ))}
              </dl>
            ) : (
              <p className="instance-detail-note transform-unavailable">该变换无法完整表示为 TRS</p>
            )}
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
