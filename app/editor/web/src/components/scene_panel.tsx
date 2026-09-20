import { useMemo, useState } from 'react';

import type { SceneObjectSummary } from '../protocol/generated';
import { SearchIcon } from './icons';

interface ScenePanelProps {
  objects: SceneObjectSummary[];
  inspectedInstanceId: string | null;
  onInspectInstance(instanceId: string): void;
}

export function ScenePanel({
  objects,
  inspectedInstanceId,
  onInspectInstance,
}: ScenePanelProps) {
  const [search, setSearch] = useState('');
  const filteredObjects = useMemo(() => {
    const normalized = search.trim().toLowerCase();
    return normalized ? objects.filter((object) => object.name.toLowerCase().includes(normalized)) : objects;
  }, [objects, search]);

  return (
    <section className="panel scene-panel" aria-labelledby="scene-title">
      <div className="panel-heading">
        <h2 id="scene-title">Scene Objects</h2>
        <span>{objects.length} objects</span>
      </div>
      <label className="search-field">
        <SearchIcon />
        <span className="sr-only">Search scene objects</span>
        <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Search instance name…" />
      </label>
      <div className="object-table" aria-label="Scene instances">
        <div className="object-row object-row--header">
          <span>Instance</span>
          <span>Materials</span>
        </div>
        <div className="object-list">
          {filteredObjects.length === 0 ? (
            <div className="empty-state">{search.trim() ? 'No matching objects.' : 'No objects in scene.'}</div>
          ) : (
            filteredObjects.map((object) => {
              const selected = inspectedInstanceId === object.instance_id;
              return (
                <button
                  className={`object-row${selected ? ' object-row--selected' : ''}`}
                  type="button"
                  key={object.instance_id}
                  aria-pressed={selected}
                  title={object.instance_id}
                  onClick={() => onInspectInstance(object.instance_id)}
                >
                  <span>
                    {object.name}
                  </span>
                  <span>{object.material_count}</span>
                </button>
              );
            })
          )}
        </div>
      </div>
    </section>
  );
}
