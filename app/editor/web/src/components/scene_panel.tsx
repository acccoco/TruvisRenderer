import { useMemo, useState } from 'react';

import type { SceneObjectSummary, SelectionDto } from '../protocol/generated';
import { ChevronIcon, SearchIcon } from './icons';

interface ScenePanelProps {
  objects: SceneObjectSummary[];
  selection: SelectionDto | null;
  pageOffset: number;
  nextOffset: number | null;
  onPreviousPage(): void;
  onNextPage(): void;
}

function shortId(id: string): string {
  return id.split(':')[1]?.slice(-8) ?? id;
}

export function ScenePanel({ objects, selection, pageOffset, nextOffset, onPreviousPage, onNextPage }: ScenePanelProps) {
  const [search, setSearch] = useState('');
  const filteredObjects = useMemo(() => {
    const normalized = search.trim().toLowerCase();
    return normalized ? objects.filter((object) => object.instance_id.toLowerCase().includes(normalized)) : objects;
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
        <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Search object ID…" />
      </label>
      <div className="object-table" role="table" aria-label="Scene objects">
        <div className="object-row object-row--header" role="row">
          <span role="columnheader">Object ID</span>
          <span role="columnheader">Materials</span>
        </div>
        <div className="object-list">
          {filteredObjects.length === 0 ? (
            <div className="empty-state">No objects on this page.</div>
          ) : (
            filteredObjects.map((object) => {
              const selected = selection?.instance_id === object.instance_id;
              return (
                <div className={`object-row${selected ? ' object-row--selected' : ''}`} role="row" key={object.instance_id}>
                  <span role="cell" title={object.instance_id}>
                    {shortId(object.instance_id)}
                  </span>
                  <span role="cell">{object.material_count}</span>
                </div>
              );
            })
          )}
        </div>
      </div>
      <div className="pagination">
        <button className="icon-button" type="button" onClick={onPreviousPage} disabled={pageOffset === 0} aria-label="Previous page">
          <ChevronIcon direction="left" />
        </button>
        <span>
          {objects.length === 0 ? '0' : `${pageOffset + 1}–${pageOffset + objects.length}`}
        </span>
        <button className="icon-button" type="button" onClick={onNextPage} disabled={nextOffset === null} aria-label="Next page">
          <ChevronIcon direction="right" />
        </button>
      </div>
    </section>
  );
}
