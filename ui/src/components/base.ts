/** Small helpers shared by the custom elements. */

export const baseCss = `
  :host {
    display: block;
    color: var(--wd-text);
    font: inherit;
  }
  *,
  *::before,
  *::after {
    box-sizing: border-box;
  }
  button,
  input,
  select {
    font: inherit;
    color: inherit;
  }
  button {
    cursor: pointer;
  }
  button:disabled,
  input:disabled,
  select:disabled {
    cursor: not-allowed;
    opacity: 0.55;
  }
  :focus-visible {
    outline: 2px solid var(--wd-focus);
    outline-offset: 2px;
  }
`;

/** Attach an encapsulated stylesheet, with a `<style>` fallback. */
export function attachStyles(root: ShadowRoot, css: string): void {
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css);
    root.adoptedStyleSheets = [...root.adoptedStyleSheets, sheet];
  } catch {
    const style = document.createElement("style");
    style.textContent = css;
    root.append(style);
  }
}

export function defineOnce(name: string, ctor: CustomElementConstructor): void {
  if (!customElements.get(name)) customElements.define(name, ctor);
}

export function emit<T>(element: HTMLElement, type: string, detail: T): void {
  element.dispatchEvent(
    new CustomEvent<T>(type, { detail, bubbles: true, composed: true }),
  );
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** Number of decimal places implied by a step such as `0.1` or `0.05`. */
export function decimalsForStep(step: number): number {
  if (!Number.isFinite(step) || step <= 0) return 0;
  const text = String(step);
  const exponent = text.match(/e-(\d+)$/i);
  if (exponent?.[1]) return Math.min(12, Number(exponent[1]));
  const dot = text.indexOf(".");
  return dot === -1 ? 0 : text.length - dot - 1;
}

/** Round a value to the nearest allowed step, clamped to the range. */
export function roundToStep(
  value: number,
  min: number,
  max: number,
  step: number,
): number {
  if (!Number.isFinite(value)) return min;
  const bounded = clamp(value, min, max);
  if (!Number.isFinite(step) || step <= 0) return bounded;
  const stepped = min + Math.round((bounded - min) / step) * step;
  const decimals = decimalsForStep(step);
  return Number(clamp(stepped, min, max).toFixed(decimals));
}

/** Compact, locale-independent number formatting for readouts. */
export function formatNumber(value: number, precision = 3): string {
  if (!Number.isFinite(value)) return "—";
  const fixed = value.toFixed(precision);
  if (!fixed.includes(".")) return fixed;
  return fixed.replace(/0+$/, "").replace(/\.$/, "");
}

export function formatValue(
  value: unknown,
  precision = 3,
  unit = "",
): string {
  let text: string;
  if (value === undefined || value === null) {
    text = "—";
  } else if (typeof value === "number") {
    text = formatNumber(value, precision);
  } else if (typeof value === "boolean") {
    text = value ? "on" : "off";
  } else if (typeof value === "string") {
    text = value;
  } else if (Array.isArray(value)) {
    const first = value[0];
    const second = value[1];
    if (
      value.length === 2 &&
      typeof first === "number" &&
      typeof second === "number"
    ) {
      text = `${formatNumber(first, precision)}–${formatNumber(second, precision)}`;
    } else if (
      first &&
      typeof first === "object" &&
      ("x" in first || "y" in first)
    ) {
      text = `${value.length} region${value.length === 1 ? "" : "s"}`;
    } else if (
      first &&
      typeof first === "object" &&
      ("score" in first || "id" in first)
    ) {
      text = `${value.length} face${value.length === 1 ? "" : "s"}`;
    } else {
      text = `${value.length} item${value.length === 1 ? "" : "s"}`;
    }
  } else if (typeof value === "object") {
    const record = value as Record<string, unknown>;
    if (
      typeof record["width"] === "number" &&
      typeof record["height"] === "number"
    ) {
      text = `${record["width"]}×${record["height"]}`;
    } else if (Array.isArray(record["red"])) {
      text = "curve";
    } else if (typeof record["r"] === "number" && typeof record["b"] === "number") {
      const green =
        typeof record["g"] === "number"
          ? formatNumber(record["g"], precision)
          : `${formatNumber(Number(record["g_even"] ?? 0), precision)}/${formatNumber(Number(record["g_odd"] ?? 0), precision)}`;
      text = `r ${formatNumber(record["r"], precision)} · g ${green} · b ${formatNumber(record["b"], precision)}`;
    } else if ("components" in record || "matrix_size" in record) {
      text = "matrix";
    } else {
      text = "set";
    }
  } else {
    text = String(value);
  }
  return unit ? `${text} ${unit}` : text;
}

export function valuesEqual(a: unknown, b: unknown, tolerance = 1e-6): boolean {
  if (typeof a === "number" && typeof b === "number") {
    return Math.abs(a - b) <= tolerance;
  }
  return JSON.stringify(a) === JSON.stringify(b);
}

export function parseNumber(value: string | null, fallback: number): number {
  if (value === null || value.trim() === "") return fallback;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}
