/** A labelled range slider with its value shown on the right. */
export function Slider({
  value,
  min,
  max,
  step,
  format,
  onChange,
  label,
  width = "w-44",
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  format: (v: number) => string;
  onChange: (v: number) => void;
  label?: string;
  width?: string;
}) {
  return (
    <label className="flex items-center gap-3 text-[12.5px]">
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-label={label}
        className={`slider ${width}`}
        style={{ ["--fill" as string]: `${((value - min) / (max - min)) * 100}%` }}
        onChange={(e) => onChange(parseFloat(e.target.value))}
      />
      <span className="tabular w-12 text-right text-fg">{format(value)}</span>
    </label>
  );
}
