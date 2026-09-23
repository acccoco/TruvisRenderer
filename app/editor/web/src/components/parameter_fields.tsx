import { useEffect, useRef, useState } from 'react';

interface NumberFieldProps {
  label: string;
  value: number;
  min?: number;
  max?: number;
  onDraft(value: number): void;
  onCommit(value: number): void;
}

/** 数值输入保留编辑中的字符串，空值和非法值不会转换成零提交。 */
export function NumberField({ label, value, min, max, onDraft, onCommit }: NumberFieldProps) {
  const [text, setText] = useState(String(value));
  const [focused, setFocused] = useState(false);
  const [invalid, setInvalid] = useState(false);
  const edited = useRef(false);
  useEffect(() => { if (!focused && !invalid) setText(String(value)); }, [focused, invalid, value]);
  return <label className="field field--row numeric-field"><span>{label}</span><input type="number" step="any"
    value={text} min={min} max={max} aria-invalid={invalid} onFocus={() => setFocused(true)}
    onChange={(event) => {
      edited.current = true;
      setText(event.target.value);
      const valid = event.target.value !== '' && event.target.validity.valid && Number.isFinite(event.target.valueAsNumber);
      setInvalid(!valid);
      if (valid) onDraft(event.target.valueAsNumber);
    }}
    onBlur={(event) => {
      setFocused(false);
      const valid = event.target.value !== '' && event.target.validity.valid && Number.isFinite(event.target.valueAsNumber);
      setInvalid(!valid);
      if (valid && edited.current) { edited.current = false; onCommit(event.target.valueAsNumber); }
    }}
    onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }} />
    {invalid && <small className="field-error">Enter a finite value within the allowed range.</small>}
  </label>;
}

export function VectorField({ label, value, min, axes = ['X', 'Y', 'Z'], onDraft, onCommit }: {
  label: string; value: [number, number, number]; min?: number; axes?: [string, string, string];
  onDraft(value: [number, number, number]): void; onCommit(value: [number, number, number]): void;
}) {
  return <fieldset className="vector-field"><legend>{label}</legend><div>
    {value.map((entry, axis) => <NumberField key={axis} label={axes[axis]} value={entry} min={min}
      onDraft={(number) => { const next = [...value] as typeof value; next[axis] = number; onDraft(next); }}
      onCommit={(number) => { const next = [...value] as typeof value; next[axis] = number; onCommit(next); }} />)}
  </div></fieldset>;
}
