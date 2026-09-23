import { useMemo, useState } from 'react';
import type { SceneObjectSummary } from '../protocol/generated';
import type { InspectedObject } from '../state/use_editor_session';
import { SearchIcon } from './icons';

interface ScenePanelProps {
  objects: SceneObjectSummary[];
  inspectedObject: InspectedObject | null;
  onInspectObject(target: InspectedObject): void;
}

export function ScenePanel({ objects, inspectedObject, onInspectObject }: ScenePanelProps) {
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<'all' | 'lights' | 'instances'>('all');
  const filtered = useMemo(() => objects.filter((object) =>
    (filter === 'all' || (filter === 'instances' ? object.type === 'instance' : object.type !== 'instance'))
    && object.name.toLowerCase().includes(search.trim().toLowerCase())), [objects, filter, search]);
  return (
    <section className="panel scene-panel" aria-labelledby="scene-title">
      <div className="panel-heading"><h2 id="scene-title">Scene Objects</h2><span>{filtered.length} / {objects.length}</span></div>
      <div className="scene-filters" role="group" aria-label="Filter scene objects">
        {(['all', 'lights', 'instances'] as const).map((value) => <button key={value} type="button" aria-pressed={filter === value}
          onClick={() => setFilter(value)}>{value === 'all' ? 'All' : value === 'lights' ? 'Lights' : 'MeshInstance'}</button>)}
      </div>
      <label className="search-field"><SearchIcon /><span className="sr-only">Search scene objects</span>
        <input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search object name…" /></label>
      <div className="object-table" aria-label="Scene objects">
        <div className="object-row object-row--header"><span>Name</span><span>Type</span></div>
        <div className="object-list">
          {filtered.length === 0 ? <div className="empty-state">No matching objects.</div> : filtered.map((object) => {
            const target: InspectedObject = object.type === 'instance' ? { type: 'instance', instance_id: object.instance_id }
              : object.type === 'environment' ? { type: 'environment' } : { type: 'light', light_id: object.light_id };
            const key = target.type === 'instance' ? target.instance_id : target.type === 'light' ? target.light_id : 'environment';
            const selected = JSON.stringify(target) === JSON.stringify(inspectedObject);
            return <button type="button" key={key} className={`object-row${selected ? ' object-row--selected' : ''}`}
              aria-pressed={selected} title={`${object.name}\n${key}`} onClick={() => onInspectObject(target)}>
              <span>{object.name}</span><span>{object.type === 'instance' ? 'Mesh' : object.type === 'environment' ? 'HDRI' : object.type}</span>
            </button>;
          })}
        </div>
      </div>
    </section>
  );
}
