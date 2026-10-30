import { useEffect, useRef, useState } from 'react';

interface NumberFieldProps {
  label: string;
  value: number;
  min?: number;
  max?: number;
  step?: number | 'any';
  /** 整数字段（如 UV set 索引）额外要求整数；其余字段不受 step 网格约束。 */
  integer?: boolean;
  className?: string;
  onDraft(value: number): void;
  onCommit(value: number): void;
}

/** step 只决定微调箭头步长；任意精度的手输值只要在 min/max 内就合法，不因 stepMismatch 拒绝。 */
function isAcceptedNumber(input: HTMLInputElement, integer: boolean): boolean {
  const { badInput, rangeUnderflow, rangeOverflow } = input.validity;
  const value = input.valueAsNumber;
  return input.value !== '' && !badInput && !rangeUnderflow && !rangeOverflow && Number.isFinite(value)
    && (!integer || Number.isInteger(value));
}

/** 数值输入保留编辑中的字符串，空值和非法值不会转换成零提交。 */
export function NumberField({ label, value, min, max, step = 'any', integer = false, className = 'field field--row numeric-field', onDraft, onCommit }: NumberFieldProps) {
  const [text, setText] = useState(String(value));
  const [focused, setFocused] = useState(false);
  const [invalid, setInvalid] = useState(false);
  const edited = useRef(false);
  useEffect(() => { if (!focused && !invalid) setText(String(value)); }, [focused, invalid, value]);
  return <label className={className}><span>{label}</span><input type="number" step={step}
    value={text} min={min} max={max} aria-invalid={invalid} onFocus={() => setFocused(true)}
    onChange={(event) => {
      edited.current = true;
      setText(event.target.value);
      const valid = isAcceptedNumber(event.target, integer);
      setInvalid(!valid);
      if (valid) onDraft(event.target.valueAsNumber);
    }}
    onBlur={(event) => {
      setFocused(false);
      const valid = isAcceptedNumber(event.target, integer);
      setInvalid(!valid);
      if (valid && edited.current) { edited.current = false; onCommit(event.target.valueAsNumber); }
    }}
    onKeyDown={(event) => { if (event.key === 'Enter') event.currentTarget.blur(); }} />
    {invalid && <small className="field-error">{integer ? 'Enter a finite integer within the allowed range.' : 'Enter a finite value within the allowed range.'}</small>}
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
