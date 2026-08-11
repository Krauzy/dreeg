interface NumericFieldProps {
  label: string;
  value: number;
  onChange: (value: number) => void;
  step?: number;
  min?: number;
  max?: number;
  compact?: boolean;
  disabled?: boolean;
}

function clamp(value: number, min?: number, max?: number) {
  return Math.min(max ?? Number.POSITIVE_INFINITY, Math.max(min ?? Number.NEGATIVE_INFINITY, value));
}

export function NumericField({
  label,
  value,
  onChange,
  step = 1,
  min,
  max,
  compact = false,
  disabled = false,
}: NumericFieldProps) {
  const safeValue = Number.isFinite(value) ? value : 0;
  const update = (next: number) => onChange(clamp(Number.isFinite(next) ? next : safeValue, min, max));
  return (
    <label className={compact ? "field numeric-field compact" : "field numeric-field"}>
      <span>{label}</span>
      <div className="number-control">
        <button type="button" disabled={disabled} aria-label={`Decrease ${label}`} onClick={() => update(safeValue - step)}>−</button>
        <input
          type="number"
          value={safeValue}
          step={step}
          min={min}
          max={max}
          disabled={disabled}
          onChange={(event) => update(event.currentTarget.valueAsNumber)}
        />
        <button type="button" disabled={disabled} aria-label={`Increase ${label}`} onClick={() => update(safeValue + step)}>+</button>
      </div>
    </label>
  );
}
