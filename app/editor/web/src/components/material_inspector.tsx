import { useEffect, useEffectEvent, useRef } from 'react';

import type { MaterialClassDto, MaterialDto, MaterialPatch } from '../protocol/generated';

interface MaterialInspectorProps {
  material: MaterialDto | null;
  dirty: boolean;
  updateDraft(patch: Partial<MaterialDto>): void;
  commitMaterial(patch: MaterialPatch): Promise<void>;
}

function completePatch(patch: Partial<MaterialPatch>): MaterialPatch {
  return {
    name: null,
    base_color: null,
    metallic: null,
    roughness: null,
    class: null,
    coverage: null,
    ...patch,
  };
}

function colorToHex(color: [number, number, number, number]): string {
  return `#${color
    .slice(0, 3)
    .map((channel) => Math.round(Math.min(1, Math.max(0, channel)) * 255).toString(16).padStart(2, '0'))
    .join('')}`;
}

function hexToRgb(hex: string): [number, number, number] {
  return [1, 3, 5].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16) / 255) as [number, number, number];
}

export function MaterialInspector({ material, dirty, updateDraft, commitMaterial }: MaterialInspectorProps) {
  if (!material) {
    return (
      <section className="panel inspector-panel" aria-labelledby="inspector-title">
        <div className="panel-heading">
          <h2 id="inspector-title">Material Inspector</h2>
        </div>
        <div className="inspector-empty">Select a material to edit its CPU World parameters.</div>
      </section>
    );
  }

  const commitClass = (classValue: MaterialClassDto) => {
    updateDraft({ class: classValue });
    void commitMaterial(completePatch({ class: classValue }));
  };
  const materialClass = material.class;
  const coverageMode = material.coverage;

  return (
    <section className="panel inspector-panel" aria-labelledby="inspector-title">
      <div className="panel-heading">
        <h2 id="inspector-title">Material Inspector</h2>
        <span className={dirty ? 'draft-state draft-state--dirty' : 'draft-state'}>
          <span className="status-dot" />
          {dirty ? 'Local draft' : 'World state'}
        </span>
      </div>

      <div className="inspector-fields">
        <label className="field field--row">
          <span>Name</span>
          <input
            value={material.name}
            onChange={(event) => updateDraft({ name: event.target.value })}
            onBlur={(event) => void commitMaterial(completePatch({ name: event.currentTarget.value }))}
          />
        </label>

        <BaseColorEditor color={material.base_color} updateDraft={updateDraft} commitMaterial={commitMaterial} />

        <RangeField
          label="Metallic"
          value={material.metallic}
          onDraft={(value) => updateDraft({ metallic: value })}
          onCommit={(value) => commitMaterial(completePatch({ metallic: value }))}
        />
        <RangeField
          label="Roughness"
          value={material.roughness}
          onDraft={(value) => updateDraft({ roughness: value })}
          onCommit={(value) => commitMaterial(completePatch({ roughness: value }))}
        />

        <label className="field field--row">
          <span>Material Class</span>
          <select
            value={materialClass.kind}
            onChange={(event) => {
              const kind = event.target.value;
              commitClass(
                kind === 'transmission'
                  ? { kind: 'transmission', opacity: 0.5, ior: 1.5 }
                  : kind === 'emissive'
                    ? { kind: 'emissive', radiance: [1, 1, 1] }
                    : { kind: 'surface' },
              );
            }}
          >
            <option value="surface">Surface</option>
            <option value="transmission">Transmission</option>
            <option value="emissive">Emissive</option>
          </select>
        </label>

        {materialClass.kind === 'transmission' ? (
          <div className="inline-fields">
            <RangeField
              label="Opacity"
              value={materialClass.opacity}
              onDraft={(value) => updateDraft({ class: { ...materialClass, opacity: value } })}
              onCommit={(value) => commitMaterial(completePatch({ class: { ...materialClass, opacity: value } }))}
            />
            <label className="field field--row">
              <span>IOR</span>
              <input
                type="number"
                min="1"
                step="0.01"
                value={materialClass.ior}
                onChange={(event) => updateDraft({ class: { ...materialClass, ior: Number(event.target.value) } })}
                onBlur={(event) =>
                  void commitMaterial(completePatch({ class: { ...materialClass, ior: Number(event.currentTarget.value) } }))
                }
              />
            </label>
          </div>
        ) : null}

        {materialClass.kind === 'emissive' ? (
          <fieldset className="field-group radiance-group">
            <legend>Emissive Radiance</legend>
            {materialClass.radiance.map((channel, index) => (
              <label className="channel-field" key={index}>
                <span>{['R', 'G', 'B'][index]}</span>
                <input
                  type="number"
                  min="0"
                  step="0.1"
                  value={channel}
                  onChange={(event) => {
                    const radiance = [...materialClass.radiance] as [number, number, number];
                    radiance[index] = Number(event.target.value);
                    updateDraft({ class: { kind: 'emissive', radiance } });
                  }}
                  onBlur={(event) => {
                    const radiance = [...materialClass.radiance] as [number, number, number];
                    radiance[index] = Number(event.currentTarget.value);
                    void commitMaterial(completePatch({ class: { kind: 'emissive', radiance } }));
                  }}
                />
              </label>
            ))}
          </fieldset>
        ) : null}

        <label className="field field--row">
          <span>Coverage</span>
          <select
            value={coverageMode.kind}
            onChange={(event) => {
              const coverage = event.target.value === 'alpha_mask' ? { kind: 'alpha_mask' as const, alpha_cutoff: 0.5 } : { kind: 'opaque' as const };
              updateDraft({ coverage });
              void commitMaterial(completePatch({ coverage }));
            }}
          >
            <option value="opaque">Opaque</option>
            <option value="alpha_mask">Alpha Mask</option>
          </select>
        </label>

        {coverageMode.kind === 'alpha_mask' ? (
          <RangeField
            label="Alpha Cutoff"
            value={coverageMode.alpha_cutoff}
            onDraft={(value) => updateDraft({ coverage: { kind: 'alpha_mask', alpha_cutoff: value } })}
            onCommit={(value) => commitMaterial(completePatch({ coverage: { kind: 'alpha_mask', alpha_cutoff: value } }))}
          />
        ) : null}

        <ReadOnlyBinding label="Diffuse Texture" value={material.diffuse_texture} />
        <ReadOnlyBinding label="Normal Texture" value={material.normal_texture} />
      </div>

      <p className="commit-note">
        <span className="status-dot" />
        Color picker changes commit when confirmed. Slider changes commit when released.
      </p>
    </section>
  );
}

interface BaseColorEditorProps {
  color: MaterialDto['base_color'];
  updateDraft(patch: Partial<MaterialDto>): void;
  commitMaterial(patch: MaterialPatch): Promise<void>;
}

/**
 * Base Color 的本地 draft 与最终提交边界。
 *
 * React 会把 type=color 的原生 input 合成为 onChange，因此不能用 React onChange
 * 区分连续预览与最终确认。这里用 onInput 更新 draft，并直接监听原生 change，确保
 * 只有用户确认最终颜色时才向 Render 发送一次 commit；StrictMode 卸载时同步移除 listener。
 */
function BaseColorEditor({ color, updateDraft, commitMaterial }: BaseColorEditorProps) {
  const colorInputRef = useRef<HTMLInputElement>(null);
  const commitNativeColorChange = useEffectEvent((event: Event) => {
    const input = event.currentTarget as HTMLInputElement;
    const [r, g, b] = hexToRgb(input.value);
    void commitMaterial(completePatch({ base_color: [r, g, b, color[3]] }));
  });

  useEffect(() => {
    const input = colorInputRef.current;
    if (!input) {
      return;
    }

    const handleNativeChange = (event: Event) => commitNativeColorChange(event);
    input.addEventListener('change', handleNativeChange);
    return () => input.removeEventListener('change', handleNativeChange);
  }, []);

  return (
    <fieldset className="field-group color-group">
      <legend>Base Color</legend>
      <input
        ref={colorInputRef}
        className="color-well"
        type="color"
        aria-label="Base color"
        value={colorToHex(color)}
        onInput={(event) => {
          const [r, g, b] = hexToRgb(event.currentTarget.value);
          updateDraft({ base_color: [r, g, b, color[3]] });
        }}
      />
      {color.map((channel, index) => (
        <label className="channel-field" key={['R', 'G', 'B', 'A'][index]}>
          <span>{['R', 'G', 'B', 'A'][index]}</span>
          <input
            type="number"
            min="0"
            max="1"
            step="0.001"
            value={channel.toFixed(3)}
            onChange={(event) => {
              const updatedColor = [...color] as MaterialDto['base_color'];
              updatedColor[index] = Number(event.target.value);
              updateDraft({ base_color: updatedColor });
            }}
            onBlur={(event) => {
              const updatedColor = [...color] as MaterialDto['base_color'];
              updatedColor[index] = Number(event.currentTarget.value);
              void commitMaterial(completePatch({ base_color: updatedColor }));
            }}
          />
        </label>
      ))}
    </fieldset>
  );
}

interface RangeFieldProps {
  label: string;
  value: number;
  onDraft(value: number): void;
  onCommit(value: number): Promise<void>;
}

function RangeField({ label, value, onDraft, onCommit }: RangeFieldProps) {
  return (
    <label className="field range-field">
      <span>{label}</span>
      <input
        type="range"
        min="0"
        max="1"
        step="0.001"
        value={value}
        onChange={(event) => onDraft(Number(event.target.value))}
        onPointerUp={(event) => void onCommit(Number(event.currentTarget.value))}
        onKeyUp={(event) => void onCommit(Number(event.currentTarget.value))}
        onBlur={(event) => void onCommit(Number(event.currentTarget.value))}
      />
      <output>{value.toFixed(3)}</output>
    </label>
  );
}

function ReadOnlyBinding({ label, value }: { label: string; value: string | null }) {
  return (
    <label className="field field--row texture-field">
      <span>{label}</span>
      <input value={value ?? 'None'} readOnly title={value ?? 'No texture bound'} />
    </label>
  );
}
