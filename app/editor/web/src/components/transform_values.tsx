/** 共用的只读 XYZ 数值展示，保持 instance 与 light Inspector 格式一致。 */
export function TransformValues({ label, values, unit = '' }: { label: string; values: number[]; unit?: string }) {
  return values.map((value, index) => {
    const axis = ['X', 'Y', 'Z'][index];
    const formatted = value.toFixed(4);
    const text = `${formatted === '-0.0000' ? '0.0000' : formatted}${unit}`;
    return (
      <div className="transform-axis" key={axis}>
        <span aria-hidden="true">{axis}</span>
        <output aria-label={`${label} ${axis}`} title={text}>{text}</output>
      </div>
    );
  });
}
