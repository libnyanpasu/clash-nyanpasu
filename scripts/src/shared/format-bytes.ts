const UNITS = "BkMGTPEZY";

/** Format a byte count with decimal units and three significant digits. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes)) {
    throw new TypeError(
      `Expected a finite number, got ${typeof bytes}: ${bytes}`,
    );
  }
  const sign = bytes < 0 ? "-" : "";
  const size = Math.abs(bytes);
  if (size < 1) return `${sign}${size} B`;

  const exponent = Math.min(
    Math.floor(Math.log10(size) / 3),
    UNITS.length - 1,
  );
  const scaled = Number((size / 1000 ** exponent).toPrecision(3));
  const unit = exponent > 0 ? `${UNITS[exponent]}B` : UNITS[0];
  return `${sign}${scaled} ${unit}`;
}
