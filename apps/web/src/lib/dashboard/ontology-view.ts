//! A read-only view of the vault's own model: what types exist, what each one
//! declares, and what may be connected to what.
//!
//! This is the model, not the data. The folder tree and the reader show what
//! *is* in the vault; this shows the rules it is held to — the same rules the
//! write boundary enforces, so a document rejected on write can be understood
//! by looking here rather than by guessing.

import { type OntologyResponse, type OntologyType, type FieldSchema } from '../api';

import { documentBody, breadcrumb, documentTitle } from './elements';

export function renderOntology(ontology: OntologyResponse) {
  breadcrumb.textContent = 'Vault model';
  documentTitle.textContent = 'Vault model';
  documentBody.className = 'reader-body ontology-view';

  const byName = new Map(ontology.types.map((type) => [type.name, type]));
  documentBody.replaceChildren(
    summary(ontology),
    section(
      'Types',
      ontology.types.map((type) => typeCard(type, ontology)),
    ),
    section('Predicates', [predicateTable(ontology)]),
    section('What may connect to what', [connectionList(ontology, byName)]),
  );
}

function summary(ontology: OntologyResponse) {
  const wrapper = document.createElement('p');
  wrapper.className = 'ontology-summary muted';
  const documents = ontology.types.reduce((total, type) => total + type.document_count, 0);
  wrapper.textContent =
    `${ontology.types.length} types, ${ontology.edges.length} predicates, ` +
    `${ontology.links.length} legal connections, ${documents} documents.`;
  return wrapper;
}

function section(title: string, children: HTMLElement[]) {
  const element = document.createElement('section');
  element.className = 'ontology-section';

  const heading = document.createElement('h2');
  heading.textContent = title;
  element.append(heading, ...children);
  return element;
}

function typeCard(type: OntologyType, ontology: OntologyResponse) {
  const card = document.createElement('article');
  card.className = 'ontology-type';

  const heading = document.createElement('h3');
  heading.textContent = type.name;
  if (type.extends) {
    const extend = document.createElement('span');
    extend.className = 'ontology-extends';
    // `extends` is "is a": a subtype satisfies every rule written for its
    // supertype, so it is the first thing to know about a type that has one.
    extend.textContent = `is a ${type.extends}`;
    heading.append(extend);
  }

  const meta = document.createElement('p');
  meta.className = 'ontology-meta muted';
  meta.textContent = describeCount(type);
  if (type.folders.length > 0) {
    meta.textContent += ` · ${type.folders.join(', ')}`;
  }

  card.append(heading, meta);

  const fields = Object.entries(type.fields);
  if (fields.length === 0) {
    // Declaring nothing is the default, and most vaults declare nothing for
    // most types. Saying so on every card is nineteen lines of noise; the
    // absence of a field table already says it.
    card.classList.add('ontology-type-bare');
  } else {
    card.append(fieldTable(fields, type.required));
  }

  const predicates = ontology.edges.filter((edge) => edge.from.includes(type.name));
  if (predicates.length > 0) {
    const outgoing = document.createElement('p');
    outgoing.className = 'ontology-meta ontology-outgoing muted';
    outgoing.textContent = `Can be the source of: ${predicates
      .map((edge) => edge.predicate)
      .join(', ')}`;
    card.append(outgoing);
  }

  return card;
}

/// Document counts, with folder indexes named rather than folded in — a folder
/// index is a document of its type, so "1 person" on a fresh vault is the
/// `people` folder itself.
function describeCount(type: OntologyType) {
  const { document_count: documents, folder_index_count: folders } = type;
  const noun = documents === 1 ? 'document' : 'documents';
  if (folders === 0) {
    return `${documents} ${noun}`;
  }
  return `${documents} ${noun} (${folders} folder ${folders === 1 ? 'index' : 'indexes'})`;
}

/// Flatten a field tree into rows, so a table's interior is visible rather than
/// collapsing to the word "table". Depth drives indentation; the name carries
/// the dotted path so a row is unambiguous read on its own.
function fieldRows(
  fields: [string, FieldSchema][],
  required: string[],
  prefix = '',
  depth = 0,
): { path: string; field: FieldSchema; required: boolean; depth: number }[] {
  return fields.flatMap(([name, field]) => {
    const path = prefix ? `${prefix}.${name}` : name;
    const row = { path, field, required: required.includes(name), depth };
    const nested = Object.entries(field.fields ?? {});
    return nested.length === 0
      ? [row]
      : [row, ...fieldRows(nested, field.required ?? [], path, depth + 1)];
  });
}

function fieldTable(fields: [string, FieldSchema][], required: string[]) {
  const table = document.createElement('table');
  table.className = 'ontology-table';
  table.append(
    headerRow(['Field', 'Type', 'Notes']),
    ...fieldRows(fields, required).map(({ path, field, required: isRequired, depth }) => {
      const row = document.createElement('tr');
      const nameCell = document.createElement('td');
      // The dotted path, which at depth 0 is just the field name.
      nameCell.textContent = path;
      if (depth > 0) {
        nameCell.style.setProperty('--depth', String(depth));
        nameCell.classList.add('ontology-nested-field');
      }
      if (isRequired) {
        const marker = document.createElement('span');
        marker.className = 'ontology-required';
        marker.textContent = 'required';
        nameCell.append(marker);
      }

      const typeCell = document.createElement('td');
      typeCell.textContent = field.items ? `${field.type} of ${field.items}` : field.type;

      const notes = document.createElement('td');
      notes.className = 'muted';
      const parts: string[] = [];
      if (field.to && field.to.length > 0) {
        parts.push(`→ ${field.to.join(', ')}`);
      }
      if (field.description) {
        parts.push(field.description);
      }
      notes.textContent = parts.join(' · ');

      row.append(nameCell, typeCell, notes);
      return row;
    }),
  );
  return table;
}

function predicateTable(ontology: OntologyResponse) {
  const table = document.createElement('table');
  table.className = 'ontology-table';
  table.append(
    headerRow(['Predicate', 'From', 'To', 'Inverse', 'Cardinality']),
    ...ontology.edges.map((edge) => {
      const row = document.createElement('tr');
      const cells = [
        edge.predicate,
        edge.from.join(', '),
        edge.to.join(', '),
        // A symmetric edge has no separate inverse: it reads the same both ways.
        edge.symmetric ? 'symmetric' : (edge.inverse ?? '—'),
        edge.cardinality ?? '—',
      ];
      for (const value of cells) {
        const cell = document.createElement('td');
        cell.textContent = value;
        row.append(cell);
      }
      if (edge.description) {
        row.title = edge.description;
      }
      return row;
    }),
  );
  return table;
}

/// The type-level graph, grouped by source. `links` is what *may* be connected;
/// the graph view shows what currently is.
function connectionList(ontology: OntologyResponse, byName: Map<string, OntologyType>) {
  const bySource = new Map<string, string[]>();
  for (const link of ontology.links) {
    const entry = bySource.get(link.source) ?? [];
    entry.push(`${link.predicate} → ${link.target}`);
    bySource.set(link.source, entry);
  }

  const list = document.createElement('div');
  list.className = 'ontology-connections';
  if (bySource.size === 0) {
    const none = document.createElement('p');
    none.className = 'muted';
    none.textContent = 'No predicates declared.';
    list.append(none);
    return list;
  }

  for (const [source, targets] of [...bySource].sort()) {
    const group = document.createElement('div');
    group.className = 'ontology-connection';

    const name = document.createElement('strong');
    // `*` means any type, which is worth spelling out rather than leaving as
    // punctuation the reader has to decode.
    name.textContent = source === '*' ? 'any type' : source;
    if (source !== '*' && !byName.has(source)) {
      // A predicate naming a type nothing defines: legal to write, impossible
      // to satisfy. Worth surfacing rather than rendering as if it were fine.
      name.append(undefinedMarker());
    }

    const targetList = document.createElement('ul');
    for (const target of targets.sort()) {
      const item = document.createElement('li');
      item.textContent = target;
      targetList.append(item);
    }

    group.append(name, targetList);
    list.append(group);
  }
  return list;
}

function undefinedMarker() {
  const marker = document.createElement('span');
  marker.className = 'ontology-warning';
  marker.textContent = 'no such type';
  return marker;
}

function headerRow(labels: string[]) {
  const row = document.createElement('tr');
  for (const label of labels) {
    const cell = document.createElement('th');
    cell.textContent = label;
    row.append(cell);
  }
  const head = document.createElement('thead');
  head.append(row);
  return head;
}
