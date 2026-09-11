//! Turning a metadata value into text and back, faithfully.
//!
//! Separate from the form itself because these are the rules, and rules are
//! worth testing without a DOM. The form is plumbing; this is where a value
//! either survives a round trip or does not.
//!
//! Every one of these existed as a defect first. A boolean rendered as the
//! empty string and came back as a deletion. An undeclared `3` went out as
//! `"3"` and changed the field's type. `["Smith, Jane"]` joined with `", "` and
//! split on `,` became two people. None of it required touching the field —
//! the form submitted every value on every save, so editing only the Markdown
//! was enough to corrupt the metadata beside it.

/// How a value appears in a text control.
///
/// Booleans and numbers included: showing them blank is what turned an
/// untouched `true` into a removal.
export function display(value: unknown): string {
  if (value === null || value === undefined) return '';
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  if (typeof value === 'string' || typeof value === 'number') return String(value);
  return '';
}

/// Read a control's text back into the value the API should receive.
///
/// `declaredType` comes from the vault's `[nodes.*]` schema when it has one.
/// Without a schema the original value decides: a field that held a number
/// stays a number, because the alternative is a form that silently retypes
/// every undeclared field it displays.
///
/// Text that does not parse as the intended type is returned as text, not as
/// `NaN` or a removal — the write boundary then refuses it by name, which is a
/// better outcome than a field quietly becoming `null` or `0`.
export function parse(raw: string, original: unknown, declaredType?: string): unknown {
  const trimmed = raw.trim();
  if (trimmed === '') return null;

  const wanted = declaredType ?? inferredType(original);
  switch (wanted) {
    case 'boolean':
      return trimmed === 'true' ? true : trimmed === 'false' ? false : trimmed;
    case 'integer': {
      const parsed = Number.parseInt(trimmed, 10);
      return Number.isFinite(parsed) && String(parsed) === trimmed ? parsed : trimmed;
    }
    case 'number': {
      const parsed = Number.parseFloat(trimmed);
      return Number.isFinite(parsed) && String(parsed) === trimmed ? parsed : trimmed;
    }
    default:
      return trimmed;
  }
}

/// What an undeclared value's type is, judged by what it already holds.
function inferredType(original: unknown): string | undefined {
  if (typeof original === 'boolean') return 'boolean';
  if (typeof original === 'number') return Number.isInteger(original) ? 'integer' : 'number';
  return undefined;
}

/// Whether a list survives being joined with `, ` and split on `,`.
///
/// It does not if any entry contains a comma: `["Smith, Jane"]` comes back as
/// two entries. Such a list is shown read-only rather than silently rewritten.
export function listRoundTrips(values: unknown): values is string[] {
  return Array.isArray(values) && values.every((entry) => !String(entry).includes(','));
}

export function displayList(values: unknown): string {
  return Array.isArray(values) ? values.map(String).join(', ') : '';
}

export function parseList(raw: string): string[] {
  return raw
    .split(',')
    .map((entry) => entry.trim())
    .filter(Boolean);
}

/// Whether a control still holds what it was given.
///
/// An unchanged field is not sent at all, which is the difference between a
/// save that writes what you edited and one that rewrites everything it
/// displayed. Structural equality is enough here: these are scalars and string
/// lists.
export function unchanged(current: unknown, original: unknown): boolean {
  if (current === null && (original === null || original === undefined)) return true;
  return JSON.stringify(current) === JSON.stringify(original);
}
