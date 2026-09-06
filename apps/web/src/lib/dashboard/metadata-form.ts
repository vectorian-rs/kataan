//! The properties panel as a form.
//!
//! Rendered from the type's `[nodes.*]` schema when it has one, so the fields a
//! vault declares are the fields you are offered — and from whatever keys the
//! document already carries when it does not.
//!
//! Only values that survive a round trip through a text input are editable.
//! Intervals, tables, arrays and references are shown read-only rather than
//! given a control that would mangle them: a form that silently flattens
//! `{ from, to }` into a string is worse than one that declines.

import { type DocumentResponse, type FieldSchema, type TomlSchemaResponse } from '../api';

import { metadataPanel } from './elements';
import { formatLabel } from './format';

/// Keys kataan owns. They are edited through their own controls, or not at all.
const RESERVED = new Set([
  'type',
  'status',
  'markdown',
  'markdown_checksum',
  'aliases',
  'labels',
  'created_by',
  'last_updated_by',
  'occurred_at',
  'created_at',
  'updated_at',
  'edges',
]);

/// Declared types this form can present as a single input and read back
/// faithfully. Everything else is structure, and structure needs the source.
const EDITABLE_TYPES = new Set(['string', 'integer', 'number', 'boolean', 'date', 'instant']);

export interface MetadataEdit {
  status?: string | null;
  aliases?: string[];
  labels?: string[];
  occurred_at?: string | null;
  fields?: Record<string, unknown>;
}

/// A control the form can read a value back out of.
interface Field {
  key: string;
  input: HTMLInputElement | HTMLSelectElement;
  /// How to turn the input's string back into what the API expects.
  read: (raw: string) => unknown;
  /// Custom sidecar key rather than one of kataan's own.
  custom: boolean;
}

let fields: Field[] = [];

export function renderMetadataForm(
  vaultDocument: DocumentResponse,
  schema: TomlSchemaResponse | undefined,
) {
  fields = [];
  const metadata = vaultDocument.metadata;
  const declared = schema?.node_schema?.fields ?? {};
  const required = new Set(schema?.node_schema?.required ?? []);

  const own = section('Properties', [
    readOnlyRow('ID', vaultDocument.id),
    readOnlyRow('Type', String(metadata.type ?? '')),
    selectRow('status', 'Status', asString(metadata.status), [
      '',
      ...(schema?.constraints.allowed_status ?? []),
    ]),
    textRow('occurred_at', 'Occurred at', asString(metadata.occurred_at), 'RFC 3339'),
    listRow('aliases', 'Aliases', metadata.aliases),
    listRow('labels', 'Labels', metadata.labels),
  ]);

  // Declared fields first and in schema order, then anything the document
  // carries that the schema does not mention — a vault with no `[nodes.*]` is
  // then simply the second case for every key.
  const names = [
    ...Object.keys(declared),
    ...Object.keys(metadata).filter((key) => !RESERVED.has(key) && !(key in declared)),
  ];

  const custom = names.map((name) =>
    customRow(name, metadata[name], declared[name], required.has(name)),
  );

  metadataPanel.className = 'metadata-sections';
  metadataPanel.replaceChildren(
    own,
    section(custom.length > 0 ? 'Fields' : 'Fields (none declared)', custom),
  );
}

/// Everything the form wants changed. Only keys whose control exists are
/// included, so a value the form declined to edit is never sent — and therefore
/// never overwritten.
export function readMetadataForm(): MetadataEdit {
  const edit: MetadataEdit = {};
  const custom: Record<string, unknown> = {};

  for (const field of fields) {
    const value = field.read(field.input.value);
    if (field.custom) {
      custom[field.key] = value;
    } else if (field.key === 'aliases' || field.key === 'labels') {
      edit[field.key] = value as string[];
    } else {
      edit[field.key as 'status' | 'occurred_at'] = value as string | null;
    }
  }
  if (Object.keys(custom).length > 0) {
    edit.fields = custom;
  }
  return edit;
}

function customRow(
  name: string,
  value: unknown,
  schema: FieldSchema | undefined,
  isRequired: boolean,
) {
  const declaredType = schema?.type;
  const editable = declaredType === undefined ? isScalar(value) : EDITABLE_TYPES.has(declaredType);

  if (!editable) {
    // Structure, or a declared type this form cannot round-trip. Showing it
    // read-only is honest; offering a text box would let a save flatten it.
    return readOnlyRow(
      formatLabel(name),
      `${describe(value)} — edit in the Markdown source`,
      isRequired,
    );
  }

  const hint =
    declaredType === 'date'
      ? '2026-08-29'
      : declaredType === 'instant'
        ? '2026-08-29T12:00:00Z'
        : '';
  const row = textRow(name, formatLabel(name), asString(value), hint, isRequired);
  const field = fields[fields.length - 1];
  field.custom = true;
  // An emptied custom field is a removal, which is what `null` means to the
  // API — as opposed to a reserved key, where empty means "unset".
  field.read = (raw) => {
    const trimmed = raw.trim();
    if (trimmed === '') return null;
    if (declaredType === 'integer') return Number.parseInt(trimmed, 10);
    if (declaredType === 'number') return Number.parseFloat(trimmed);
    if (declaredType === 'boolean') return trimmed === 'true';
    return trimmed;
  };
  return row;
}

function textRow(key: string, label: string, value: string, placeholder = '', isRequired = false) {
  const input = document.createElement('input');
  input.type = 'text';
  input.className = 'metadata-input';
  input.value = value;
  input.placeholder = placeholder;
  fields.push({ key, input, read: (raw) => raw.trim() || null, custom: false });
  return labelled(label, input, isRequired);
}

function selectRow(key: string, label: string, value: string, options: string[]) {
  const input = document.createElement('select');
  input.className = 'metadata-input';
  for (const option of options) {
    const element = document.createElement('option');
    element.value = option;
    element.textContent = option === '' ? '—' : option;
    input.append(element);
  }
  input.value = value;
  fields.push({ key, input, read: (raw) => raw || null, custom: false });
  return labelled(label, input, false);
}

function listRow(key: string, label: string, value: unknown) {
  const items = Array.isArray(value) ? value.map(String) : [];
  const input = document.createElement('input');
  input.type = 'text';
  input.className = 'metadata-input';
  input.value = items.join(', ');
  input.placeholder = 'comma separated';
  fields.push({
    key,
    input,
    read: (raw) =>
      raw
        .split(',')
        .map((entry) => entry.trim())
        .filter(Boolean),
    custom: false,
  });
  return labelled(label, input, false);
}

function readOnlyRow(label: string, value: string, isRequired = false) {
  const shown = document.createElement('div');
  shown.className = 'metadata-readonly muted';
  shown.textContent = value || '—';
  return labelled(label, shown, isRequired);
}

function labelled(text: string, control: HTMLElement, isRequired: boolean) {
  const row = document.createElement('label');
  row.className = 'metadata-field';

  const name = document.createElement('span');
  name.className = 'property-label';
  name.textContent = text;
  if (isRequired) {
    const marker = document.createElement('span');
    marker.className = 'ontology-required';
    marker.textContent = 'required';
    name.append(marker);
  }

  row.append(name, control);
  return row;
}

function section(title: string, children: HTMLElement[]) {
  const element = document.createElement('section');
  element.className = 'metadata-section';

  const heading = document.createElement('h3');
  heading.className = 'section-heading';
  heading.textContent = title;

  const body = document.createElement('div');
  body.className = 'metadata-grid';
  body.replaceChildren(...children);

  element.append(heading, body);
  return element;
}

function isScalar(value: unknown) {
  return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean';
}

function describe(value: unknown) {
  if (Array.isArray(value)) return `${value.length} entries`;
  if (value && typeof value === 'object') return 'table';
  return asString(value) || '—';
}

function asString(value: unknown) {
  return typeof value === 'string' || typeof value === 'number' ? String(value) : '';
}
