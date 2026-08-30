import { type DocumentResponse, type TomlSchemaResponse } from '../api';

import { metadataPanel, schemaPanel } from './elements';

import { emptySectionNote, formatLabel, renderPropertyValue, schemaLine } from './format';

export function renderSchema(schema: TomlSchemaResponse) {
  schemaPanel.className = 'schema-content';

  const templateLabel = document.createElement('div');
  templateLabel.className = 'property-label';
  templateLabel.textContent = 'minimum TOML';

  const template = document.createElement('pre');
  template.className = 'schema-template';
  template.textContent = schema.toml_template.trim();

  const details = document.createElement('div');
  details.className = 'schema-details';
  details.replaceChildren(
    schemaLine('Required', requiredFields(schema).join(', ') || '—'),
    schemaLine('Types', schema.constraints.allowed_types.join(', ') || '—'),
    schemaLine('Status', schema.constraints.allowed_status.join(', ') || '—'),
    schemaLine('Actors', schema.constraints.allowed_actors.join(', ') || '—'),
    schemaLine('Edges', schema.constraints.allowed_edge_predicates.join(', ') || '—'),
  );

  schemaPanel.replaceChildren(templateLabel, template, details);
}

export function requiredFields(schema: TomlSchemaResponse) {
  const root = schema.schema as { schema?: { required?: string[] }; required?: string[] };
  return root.schema?.required ?? root.required ?? [];
}

/// `onSelectEdge` is injected rather than imported: following an edge is a
/// selection concern, and reaching back into the dashboard from here would
/// make the two modules circular.
export function renderMetadata(
  vaultDocument: DocumentResponse,
  onSelectEdge: (id: string) => void,
) {
  const {
    edges,
    markdown,
    markdown_checksum: markdownChecksum,
    ...properties
  } = vaultDocument.metadata;
  metadataPanel.className = 'metadata-sections';
  metadataPanel.replaceChildren(
    metadataSection('Properties', [
      property('ID', vaultDocument.id),
      ...Object.entries(properties).map(([key, value]) => property(formatLabel(key), value)),
    ]),
    metadataSection('Edges', renderEdges(edges, onSelectEdge)),
    metadataSection('Internal', [
      property('Markdown', markdown),
      property('Markdown checksum', markdownChecksum),
      property('Route token', vaultDocument.route_token),
    ]),
  );
}

export function metadataSection(title: string, children: HTMLElement[]) {
  const section = document.createElement('section');
  section.className = 'metadata-section';

  const heading = document.createElement('h3');
  heading.className = 'section-heading';
  heading.textContent = title;

  const body = document.createElement('div');
  body.className = 'metadata-grid';
  if (children.length === 0) {
    body.replaceChildren(emptySectionNote('None.'));
  } else {
    body.replaceChildren(...children);
  }

  section.append(heading, body);
  return section;
}

export function renderEdges(edges: unknown, onSelectEdge: (id: string) => void) {
  if (!edges || typeof edges !== 'object' || Array.isArray(edges)) {
    return [];
  }

  return Object.entries(edges as Record<string, unknown>)
    .filter(([, targets]) => Array.isArray(targets) && targets.length > 0)
    .map(([predicate, targets]) => edgeGroup(predicate, targets as unknown[], onSelectEdge));
}

export function edgeGroup(
  predicate: string,
  targets: unknown[],
  onSelectEdge: (id: string) => void,
) {
  const wrapper = document.createElement('div');
  wrapper.className = 'edge-group';

  const label = document.createElement('div');
  label.className = 'property-label';
  label.textContent = formatLabel(predicate);

  const list = document.createElement('div');
  list.className = 'edge-list';
  list.replaceChildren(...targets.map((target) => edgeTarget(target, onSelectEdge)));

  wrapper.append(label, list);
  return wrapper;
}

export function edgeTarget(target: unknown, onSelectEdge: (id: string) => void) {
  const item = document.createElement('button');
  item.className = 'edge-target';
  item.type = 'button';
  item.textContent = String(target);
  item.addEventListener('click', () => onSelectEdge(String(target)));
  return item;
}

export function property(label: string, value: unknown) {
  const wrapper = document.createElement('div');
  wrapper.className = 'property';

  const labelEl = document.createElement('div');
  labelEl.className = 'property-label';
  labelEl.textContent = label;

  const valueEl = renderPropertyValue(value);

  wrapper.append(labelEl, valueEl);
  return wrapper;
}
