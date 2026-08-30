import { formatValue } from './format';

//! Small element factories shared by the panels and list views.

export function renderPropertyValue(value: unknown) {
  const valueEl = document.createElement('div');
  valueEl.className = 'property-value';

  if (Array.isArray(value) && value.length > 0) {
    valueEl.classList.add('pill-list');
    valueEl.replaceChildren(...value.map(renderPill));
    return valueEl;
  }

  valueEl.textContent = formatValue(value);
  return valueEl;
}

export function renderPill(value: unknown) {
  const pill = document.createElement('span');
  pill.className = 'pill';
  pill.textContent = String(value);
  return pill;
}

export function clickableRow(className: string) {
  const row = document.createElement('div');
  row.className = className;
  row.role = 'button';
  row.tabIndex = 0;
  row.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      row.click();
    }
  });
  return row;
}

export function emptySectionNote(text: string) {
  const note = document.createElement('div');
  note.className = 'empty-section-note muted';
  note.textContent = text;
  return note;
}

export function emptyListNote(text: string) {
  const note = document.createElement('div');
  note.className = 'muted empty-list-note';
  note.textContent = text;
  return note;
}

export function listSection(title: string, rows: HTMLElement[]) {
  const section = document.createElement('section');
  section.className = 'content-list-section';

  const heading = document.createElement('h3');
  heading.className = 'section-heading';
  heading.textContent = title;

  const list = document.createElement('div');
  list.className = 'list';
  list.replaceChildren(...rows);

  section.append(heading, list);
  return section;
}

export function schemaLine(label: string, value: string) {
  const row = document.createElement('div');
  row.className = 'schema-line';
  const labelEl = document.createElement('span');
  labelEl.textContent = label;
  const valueEl = document.createElement('span');
  valueEl.textContent = value;
  row.append(labelEl, valueEl);
  return row;
}
