import {
  Archive,
  BookOpen,
  Boxes,
  Circle,
  Code,
  FileText,
  FolderKanban,
  Inbox,
  Lightbulb,
  ListTodo,
  Newspaper,
  Presentation,
  ReceiptText,
  User,
  createElement,
  type IconNode,
} from 'lucide';
import { type FolderFile } from '../api';

export function cssEscape(value: string) {
  return CSS.escape(value);
}

export function basenameFromId(id: string) {
  return id.split('/').at(-1) ?? id;
}

export function formatLabel(label: string) {
  return label.replaceAll('_', ' ');
}

export function formatValue(value: unknown): string {
  if (Array.isArray(value)) {
    return value.length === 0 ? '—' : value.join(', ');
  }

  if (value === null || value === undefined || value === '') {
    return '—';
  }

  if (typeof value === 'object') {
    return JSON.stringify(value, null, 2);
  }

  return String(value);
}

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

export function depthFor(id: string) {
  return Math.max(0, id.split('/').length - 1);
}

export function folderTitleFromResponse(id: string, metadata?: Record<string, unknown>) {
  const name = metadata?.name;
  return typeof name === 'string' && name.length > 0 ? name : basenameFromId(id);
}

export function normalizeFolderIconKey(typeOrFolder: string) {
  const aliases: Record<string, string> = {
    intake: 'intake',
    notes: 'note',
    people: 'person',
    projects: 'project',
    topics: 'topic',
    articles: 'Newspaper',
    presentations: 'Presentation',
    references: 'BookOpen',
    finances: 'ReceiptText',
    tasks: 'ListTodo',
    type: 'type-definition',
  };
  return aliases[typeOrFolder] ?? typeOrFolder;
}

export function folderIcon(typeOrFolder: string): IconNode {
  const key = normalizeFolderIconKey(typeOrFolder);
  const icons: Record<string, IconNode> = {
    code: Code,
    note: FileText,
    person: User,
    project: FolderKanban,
    intake: Inbox,
    raw: Archive,
    topic: Lightbulb,
    'type-definition': Boxes,
    Inbox,
    Rocket: FolderKanban,
    Newspaper,
    Presentation,
    BookOpen,
    ReceiptText,
    ListTodo,
    FolderKanban,
    FileText,
    User,
    Code,
    Boxes,
    Lightbulb,
  };
  return icons[key] ?? Circle;
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

export function fileExtensionClass(extension: string | undefined) {
  const normalized = extension?.toLowerCase().replace(/[^a-z0-9-]/g, '-');
  return normalized ? `file-ext-${normalized}` : 'file-ext-none';
}

export function isHighlightableFile(file: FolderFile) {
  return new Set([
    'bash',
    'c',
    'cc',
    'cpp',
    'cxx',
    'h',
    'hpp',
    'hs',
    'hxx',
    'js',
    'json',
    'md',
    'py',
    'rs',
    'sh',
    'toml',
    'ts',
    'txt',
    'yaml',
    'yml',
  ]).has(file.extension?.toLowerCase() ?? '');
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
