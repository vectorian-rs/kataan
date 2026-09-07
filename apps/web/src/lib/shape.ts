//! Checking that a response is the shape we think it is, at the boundary.
//!
//! TypeScript types are erased at run time, so `response.json() as Promise<T>`
//! is an assertion, not a check. If the server's shape ever moved, `astro
//! check` would still pass — the types here are hand-written, not derived from
//! the Rust structs — and the first sign of trouble would be `undefined`
//! reaching the DOM somewhere far from the cause.
//!
//! A shape is defined once and used twice: it validates the response, and the
//! TypeScript type is inferred from it with `Infer`. So there is no second
//! declaration to drift from the first.
//!
//! Deliberately forgiving in one direction: **unknown keys are allowed**. The
//! server may add a field without this app knowing about it, and refusing the
//! response for that would turn a compatible change into an outage. What is
//! declared must be present and the right type; everything else is ignored.

/// A checker: returns the value typed, or throws naming the path that was
/// wrong.
export type Shape<T> = (value: unknown, path: string) => T;

export type Infer<S> = S extends Shape<infer T> ? T : never;

export class ShapeError extends Error {}

function fail(path: string, expected: string, got: unknown): never {
  const actual = got === null ? 'null' : Array.isArray(got) ? 'array' : typeof got;
  throw new ShapeError(`${path}: expected ${expected}, got ${actual}`);
}

export const string: Shape<string> = (value, path) =>
  typeof value === 'string' ? value : fail(path, 'string', value);

export const number: Shape<number> = (value, path) =>
  typeof value === 'number' && Number.isFinite(value) ? value : fail(path, 'number', value);

export const boolean: Shape<boolean> = (value, path) =>
  typeof value === 'boolean' ? value : fail(path, 'boolean', value);

/// A JSON object with values this app does not model.
export const record: Shape<Record<string, unknown>> = (value, path) =>
  isObject(value) ? value : fail(path, 'object', value);

/// Absent, `null`, or the inner shape. Rust's `Option<T>` serializes as an
/// absent key or a `null`, so both mean the same thing here.
export function optional<T>(inner: Shape<T>): Shape<T | undefined> {
  return (value, path) => (value === undefined || value === null ? undefined : inner(value, path));
}

export function array<T>(inner: Shape<T>): Shape<T[]> {
  return (value, path) =>
    Array.isArray(value)
      ? value.map((item, index) => inner(item, `${path}[${index}]`))
      : fail(path, 'array', value);
}

/// One of a fixed set of strings, matching a Rust enum.
export function literals<const T extends readonly string[]>(...allowed: T): Shape<T[number]> {
  return (value, path) =>
    typeof value === 'string' && (allowed as readonly string[]).includes(value)
      ? (value as T[number])
      : fail(path, allowed.map((one) => `"${one}"`).join(' | '), value);
}

/// An object whose keys are data — a map of name to value — rather than a
/// fixed set of fields.
export function mapOf<T>(inner: Shape<T>): Shape<Record<string, T>> {
  return (value, path) => {
    if (!isObject(value)) return fail(path, 'object', value);
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, inner(item, `${path}.${key}`)]),
    );
  };
}

type Fields = Record<string, Shape<unknown>>;
type FromFields<F extends Fields> = { [K in keyof F]: Infer<F[K]> };

export function object<F extends Fields>(fields: F): Shape<FromFields<F>> {
  return (value, path) => {
    if (!isObject(value)) return fail(path, 'object', value);
    const checked: Record<string, unknown> = {};
    for (const [key, shape] of Object.entries(fields)) {
      checked[key] = shape(value[key], path ? `${path}.${key}` : key);
    }
    // Unknown keys are carried through rather than dropped: a caller may hand
    // the object back to the API, and silently losing a field the server sent
    // would be a worse failure than the one this guards against.
    return { ...value, ...checked } as FromFields<F>;
  };
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
