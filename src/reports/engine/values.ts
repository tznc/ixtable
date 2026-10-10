/** Values of text components: running sums and conditional formatting (PRD §15, Phase 6). */
import { evaluate, evaluateBoolean, formatValue } from "../../expr";
import type {
  CalculatedComponent,
  ConditionStyle,
  FieldComponent,
  StaticTextComponent,
} from "../types";
import type { RenderContext } from "./render";

export const ERROR_TEXT = "#Error";
export const RUNNING_NOT_NUMBER = "A running sum needs a number";

export type TextComponent = StaticTextComponent | FieldComponent | CalculatedComponent;

/** Accumulated value of one running-sum component at one band instance. */
export type RunningValue = { value: number } | { error: string };

const message = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Sum to 15 significant digits, like `src/expr` arithmetic. */
const add = (a: number, b: number) => {
  const n = a + b;
  return n === 0 ? 0 : Number(n.toPrecision(15));
};

/**
 * Adds one instance's value of a running-sum component to `previous`. Null
 * adds nothing, booleans count as 1 or 0, and anything else that isn't a
 * number is an error that stays until the sum restarts.
 */
export function accumulate(
  previous: RunningValue | undefined,
  c: FieldComponent | CalculatedComponent,
  scope: Record<string, unknown>,
  now?: Date,
): RunningValue {
  if (previous && "error" in previous) return previous;
  const sum = previous?.value ?? 0;
  if (!c.expression.trim()) return { value: sum };
  let value: unknown;
  try {
    value = evaluate(c.expression, scope, { now });
  } catch (error) {
    return { error: message(error) };
  }
  if (value === null || value === undefined) return { value: sum };
  if (typeof value === "boolean") return { value: add(sum, value ? 1 : 0) };
  if (typeof value === "number" && Number.isFinite(value)) return { value: add(sum, value) };
  return { error: RUNNING_NOT_NUMBER };
}

/**
 * Value and printed text of a text component. A running sum takes its value
 * from `ctx.running` when the band instance carries one (detail and group
 * bands); elsewhere it prints the plain value. Errors print `#Error` and add
 * a diagnostic.
 */
export function componentValue(
  c: TextComponent,
  ctx: RenderContext,
): { value: unknown; text: string } {
  if (c.kind === "staticText") return { value: c.text, text: c.text };
  const running = c.runningSum ? ctx.running?.get(c.id) : undefined;
  try {
    if (running && "error" in running) throw new Error(running.error);
    if (!running && !c.expression.trim()) return { value: null, text: "" };
    const value = running ? running.value : evaluate(c.expression, ctx.scope, { now: ctx.now });
    return { value, text: formatValue(value, c.format || undefined) };
  } catch (error) {
    ctx.diagnose(c.id, message(error));
    return { value: null, text: ERROR_TEXT };
  }
}

/** Style of the first conditional formatting rule whose `when` holds, else null. */
export function conditionStyle(
  c: TextComponent,
  value: unknown,
  ctx: RenderContext,
): ConditionStyle | null {
  for (const rule of c.conditions ?? []) {
    if (!rule.when.trim()) continue;
    try {
      if (evaluateBoolean(rule.when, { ...ctx.scope, value }, { now: ctx.now })) return rule.style;
    } catch (error) {
      ctx.diagnose(c.id, `Condition: ${message(error)}`);
    }
  }
  return null;
}

/** The component with its matching condition's style applied, and the text it prints. */
export function styledComponent<C extends TextComponent>(
  c: C,
  ctx: RenderContext,
): { component: C; text: string } {
  const { value, text } = componentValue(c, ctx);
  const style = c.conditions?.length ? conditionStyle(c, value, ctx) : null;
  if (!style) return { component: c, text };
  const merged = { ...c.style };
  if (style.bold !== undefined) merged.bold = style.bold;
  if (style.gray !== undefined) merged.gray = style.gray;
  if (style.fill !== undefined) merged.fill = style.fill;
  return { component: { ...c, style: merged }, text };
}
