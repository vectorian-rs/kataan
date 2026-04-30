import {
  Archive,
  Boxes,
  Circle,
  Code,
  File,
  FileText,
  FolderKanban,
  Inbox,
  Lightbulb,
  ListTodo,
  Newspaper,
  Presentation,
  ReceiptText,
  BookOpen,
  User,
  createElement,
  type IconNode,
} from 'lucide';
import { marked } from 'marked';

import {
  getDocument,
  getFile,
  getFolder,
  getHighlightedFile,
  getFolders,
  getSchema,
  getVault,
  rebuildIndexes,
  resolveRoute,
  validateVault,
  type Diagnostic,
  type DocumentResponse,
  type FolderChild,
  type FolderDocument,
  type FileResponse,
  type FolderFile,
  type FolderSummary,
  type TomlSchemaResponse,
  type ValidateResponse,
} from './api';

const vaultSummary = requireElement<HTMLElement>('vault-summary');
const foldersEl = requireElement<HTMLElement>('folders');
const documentsEl = requireElement<HTMLElement>('documents');
const diagnosticsEl = requireElement<HTMLElement>('diagnostics');
const folderTitle = requireElement<HTMLElement>('folder-title');
const breadcrumb = requireElement<HTMLElement>('breadcrumb');
const documentTitle = requireElement<HTMLElement>('document-title');
const documentBody = requireElement<HTMLElement>('document-body');
const metadataPanel = requireElement<HTMLElement>('metadata-panel');
const schemaPanel = requireElement<HTMLElement>('schema-panel');
const validateButton = requireElement<HTMLButtonElement>('validate-button');
const rebuildButton = requireElement<HTMLButtonElement>('rebuild-button');

let selectedFolder: string | null = null;
let selectedDocument: string | null = null;
const loadedFolderIds = new Set<string>();

validateButton.addEventListener('click', async () => {
  await runAction(async () => {
    renderDiagnostics(await validateVault());
  });
});

rebuildButton.addEventListener('click', async () => {
  await runAction(async () => {
    await rebuildIndexes();
    await loadFolders();
    renderDiagnostics(await validateVault());
  });
});

await runAction(async () => {
  await loadVault();
  await loadFolders();
  await restoreRouteSelection();
});

async function loadVault() {
  const vault = await getVault();
  vaultSummary.replaceChildren();

  const name = document.createElement('span');
  name.className = 'vault-name';
  name.textContent = vault.name;

  const separator = document.createElement('span');
  separator.className = 'vault-separator';
  separator.textContent = '·';

  const schema = document.createElement('span');
  schema.className = 'vault-schema';
  schema.textContent = `schema ${vault.schema_version}`;

  vaultSummary.append(name, separator, schema);
}

async function loadFolders() {
  const response = await getFolders();
  foldersEl.replaceChildren(...response.folders.map(renderFolderButton));

  const firstNonEmptyFolder = response.folders.find((folder) => folder.document_count > 0);
  if (!currentRouteLocator() && response.folders.length > 0) {
    await selectFolder(firstNonEmptyFolder?.folder ?? response.folders[0].folder);
  }
}

function renderFolderButton(folder: FolderSummary) {
  const button = clickableRow('nav-row');
  button.dataset.folder = folder.folder;

  const label = document.createElement('span');
  label.className = 'folder-name';

  const icon = document.createElement('span');
  icon.className = `folder-icon ${folder.type}`;
  icon.append(createElement(folderIcon(folder.icon ?? folder.type), { width: 18, height: 18, 'stroke-width': 2 }));

  const name = document.createElement('span');
  name.textContent = folder.name ?? folder.folder;

  label.append(icon, name);

  const badge = document.createElement('span');
  badge.className = 'badge';
  badge.textContent = String(folder.document_count);

  button.append(label, badge);
  button.addEventListener('click', () => runAction(() => selectFolder(folder.folder)));
  return button;
}

async function selectFolder(folder: string, options: { selectFirst?: boolean } = {}) {
  const selectFirst = options.selectFirst ?? true;
  selectedFolder = folder;
  updateActiveRows();

  const response = await getFolder(folder);
  loadedFolderIds.add(folder);
  folderTitle.textContent = folderTitleFromResponse(response.id, response.metadata);
  renderChildFolders(folder, response.folders);

  renderFolderContents(response.documents, response.files, response.folders.length > 0);
  if (response.documents.length === 0) {
    selectedDocument = null;
    updateActiveRows();
    return;
  }
  if (selectFirst) {
    await selectDocument(response.documents[0].id);
  }
}

function renderChildFolders(parentId: string, folders: FolderChild[]) {
  const parentRow = foldersEl.querySelector<HTMLElement>(`[data-folder="${cssEscape(parentId)}"]`);
  if (!parentRow) return;

  let insertAfter = parentRow;
  for (const folder of folders) {
    let row = foldersEl.querySelector<HTMLElement>(`[data-folder="${cssEscape(folder.id)}"]`);
    if (!row) {
      row = renderChildFolderButton(folder, depthFor(folder.id));
      insertAfter.after(row);
    }
    insertAfter = row;
  }
}

function renderChildFolderButton(folder: FolderChild, depth: number) {
  const button = clickableRow('nav-row nested');
  button.dataset.folder = folder.id;
  button.style.setProperty('--depth', String(depth));

  const label = document.createElement('span');
  label.className = 'folder-name';

  const icon = document.createElement('span');
  icon.className = 'folder-icon';
  icon.append(createElement(folderIcon(folder.id.split('/')[0] ?? ''), { width: 18, height: 18, 'stroke-width': 2 }));

  const name = document.createElement('span');
  name.textContent = folder.name;

  label.append(icon, name);
  button.append(label);
  button.addEventListener('click', () => runAction(() => selectFolder(folder.id)));
  return button;
}

function renderFolderContents(documents: FolderDocument[], files: FolderFile[], hasChildFolders: boolean) {
  const children: HTMLElement[] = [];

  children.push(listSection('Documents', documents.length > 0 ? documents.map(renderDocumentButton) : [emptyListNote(hasChildFolders ? 'Select a nested folder or open a file.' : 'No documents.')]));
  children.push(listSection('Files', files.length > 0 ? files.map(renderFileRow) : [emptyListNote('No files.')]));

  documentsEl.className = 'folder-contents';
  documentsEl.replaceChildren(...children);
}

function listSection(title: string, rows: HTMLElement[]) {
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

function emptyListNote(text: string) {
  const note = document.createElement('div');
  note.className = 'muted empty-list-note';
  note.textContent = text;
  return note;
}

function renderDocumentButton(vaultDocument: FolderDocument) {
  const button = clickableRow('document-row');
  button.dataset.document = vaultDocument.id;

  const title = document.createElement('strong');
  title.textContent = titleFromSlug(vaultDocument.slug);

  const meta = document.createElement('span');
  meta.className = 'muted';
  meta.textContent = vaultDocument.id;

  button.append(title, meta);
  button.addEventListener('click', () => runAction(() => selectDocument(vaultDocument.id)));
  return button;
}

function renderFileRow(file: FolderFile) {
  const row = clickableRow('file-row');

  const title = document.createElement('strong');
  title.textContent = file.name;

  const meta = document.createElement('span');
  meta.className = 'muted';
  meta.textContent = file.extension ? `${file.extension.toUpperCase()} · ${file.path}` : file.path;

  const icon = document.createElement('span');
  icon.className = 'file-icon';
  icon.append(createElement(File, { width: 16, height: 16, 'stroke-width': 2 }));

  const text = document.createElement('span');
  text.className = 'file-text';
  text.append(title, meta);

  row.append(icon, text);
  row.addEventListener('click', () => runAction(() => selectFile(file)));
  return row;
}

async function selectFile(file: FolderFile) {
  selectedDocument = null;
  updateActiveRows();
  try {
    const highlighted = await getHighlightedFile(file.path);
    breadcrumb.textContent = highlighted.path.replaceAll('/', ' › ');
    documentTitle.textContent = highlighted.name;
    renderHighlightedFile(highlighted.html);
  } catch {
    const vaultFile = await getFile(file.path);
    breadcrumb.textContent = vaultFile.path.replaceAll('/', ' › ');
    documentTitle.textContent = vaultFile.name;
    renderFileBody(vaultFile);
  }
}

function renderHighlightedFile(html: string) {
  documentBody.className = 'reader-body file-reader-body';
  documentBody.innerHTML = html;
}

function renderFileBody(file: FileResponse) {
  documentBody.className = 'reader-body';
  documentBody.innerHTML = '';

  if (file.kind === 'json') {
    const pre = document.createElement('pre');
    pre.className = 'file-preview json-preview';
    try {
      pre.textContent = JSON.stringify(JSON.parse(file.content), null, 2);
    } catch {
      pre.textContent = file.content;
    }
    documentBody.append(pre);
    return;
  }

  if (file.kind === 'text') {
    const pre = document.createElement('pre');
    pre.className = 'file-preview';
    pre.textContent = file.content;
    documentBody.append(pre);
    return;
  }

  documentBody.className = 'reader-body empty-state';
  const card = document.createElement('div');
  card.className = 'empty-card';
  const title = document.createElement('strong');
  title.textContent = 'Preview not available';
  const detail = document.createElement('span');
  detail.textContent = file.path;
  card.append(title, detail);
  documentBody.append(card);
}

async function selectDocument(id: string, options: { updateUrl?: boolean } = {}) {
  const updateUrl = options.updateUrl ?? true;
  selectedDocument = id;
  updateActiveRows();

  const vaultDocument = await getDocument(id);
  breadcrumb.textContent = vaultDocument.id.replaceAll('/', ' › ');
  documentTitle.textContent = titleFromId(vaultDocument.id);
  if (updateUrl) {
    updateRouteUrl(vaultDocument);
  }
  renderDocumentBody(vaultDocument);
  renderMetadata(vaultDocument);
  renderSchema(await getSchema('document'));
}

async function restoreRouteSelection() {
  const locator = currentRouteLocator();
  if (!locator) return;

  const resolved = await resolveRoute(locator.type, locator.token);
  if (resolved.is_folder_index) {
    await selectFolder(resolved.id, { selectFirst: false });
    return;
  }

  for (const folder of folderChain(resolved.folder)) {
    await selectFolder(folder, { selectFirst: false });
  }
  await selectDocument(resolved.id, { updateUrl: false });
}

function currentRouteLocator() {
  const [, type, token, extra] = window.location.pathname.split('/');
  if (!type || !token || extra) return null;
  if (!/^[0-9a-f]{32}$/.test(token)) return null;
  return { type, token };
}

function updateRouteUrl(vaultDocument: DocumentResponse) {
  const nextPath = `/${encodeURIComponent(vaultDocument.type_folder)}/${vaultDocument.route_token}`;
  if (window.location.pathname !== nextPath) {
    window.history.pushState({}, '', nextPath);
  }
}

function folderChain(folder: string) {
  const parts = folder.split('/').filter(Boolean);
  return parts.map((_, index) => parts.slice(0, index + 1).join('/'));
}

function renderDocumentBody(vaultDocument: DocumentResponse) {
  documentBody.className = 'reader-body';
  documentBody.innerHTML = '';

  documentBody.innerHTML = marked.parse(vaultDocument.markdown, { async: false });
}

function renderSchema(schema: TomlSchemaResponse) {
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

function schemaLine(label: string, value: string) {
  const row = document.createElement('div');
  row.className = 'schema-line';
  const labelEl = document.createElement('span');
  labelEl.textContent = label;
  const valueEl = document.createElement('span');
  valueEl.textContent = value;
  row.append(labelEl, valueEl);
  return row;
}

function requiredFields(schema: TomlSchemaResponse) {
  const root = schema.schema as { schema?: { required?: string[] }; required?: string[] };
  return root.schema?.required ?? root.required ?? [];
}

function renderMetadata(vaultDocument: DocumentResponse) {
  const { edges, markdown, markdown_checksum: markdownChecksum, ...properties } = vaultDocument.metadata;
  metadataPanel.className = 'metadata-sections';
  metadataPanel.replaceChildren(
    metadataSection('Properties', [
      property('ID', vaultDocument.id),
      ...Object.entries(properties).map(([key, value]) => property(formatLabel(key), value)),
    ]),
    metadataSection('Edges', renderEdges(edges)),
    metadataSection('Internal', [
      property('Markdown', markdown),
      property('Markdown checksum', markdownChecksum),
      property('Route token', vaultDocument.route_token),
    ]),
  );
}

function metadataSection(title: string, children: HTMLElement[]) {
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

function emptySectionNote(text: string) {
  const note = document.createElement('div');
  note.className = 'empty-section-note muted';
  note.textContent = text;
  return note;
}

function renderEdges(edges: unknown) {
  if (!edges || typeof edges !== 'object' || Array.isArray(edges)) {
    return [];
  }

  return Object.entries(edges as Record<string, unknown>)
    .filter(([, targets]) => Array.isArray(targets) && targets.length > 0)
    .map(([predicate, targets]) => edgeGroup(predicate, targets as unknown[]));
}

function edgeGroup(predicate: string, targets: unknown[]) {
  const wrapper = document.createElement('div');
  wrapper.className = 'edge-group';

  const label = document.createElement('div');
  label.className = 'property-label';
  label.textContent = formatLabel(predicate);

  const list = document.createElement('div');
  list.className = 'edge-list';
  list.replaceChildren(...targets.map(edgeTarget));

  wrapper.append(label, list);
  return wrapper;
}

function edgeTarget(target: unknown) {
  const item = document.createElement('button');
  item.className = 'edge-target';
  item.type = 'button';
  item.textContent = String(target);
  item.addEventListener('click', () => runAction(() => selectDocument(String(target))));
  return item;
}

function property(label: string, value: unknown) {
  const wrapper = document.createElement('div');
  wrapper.className = 'property';

  const labelEl = document.createElement('div');
  labelEl.className = 'property-label';
  labelEl.textContent = label;

  const valueEl = renderPropertyValue(value);

  wrapper.append(labelEl, valueEl);
  return wrapper;
}

async function runAction(action: () => Promise<void>) {
  try {
    await action();
  } catch (error) {
    renderError(error);
  }
}

function renderDiagnostics(report: ValidateResponse) {
  if (report.diagnostics.length === 0) {
    diagnosticsEl.className = 'list muted';
    diagnosticsEl.textContent = report.ok ? 'Vault is valid.' : 'No diagnostics.';
    return;
  }

  diagnosticsEl.className = 'list';
  diagnosticsEl.replaceChildren(...report.diagnostics.map(renderDiagnostic));
}

function renderDiagnostic(diagnostic: Diagnostic) {
  const item = document.createElement('div');
  item.className = `diagnostic ${diagnostic.severity}`;
  item.textContent = `${diagnostic.severity.toUpperCase()} [${diagnostic.code}] ${diagnostic.path ?? ''} ${diagnostic.message}`;
  return item;
}

function renderError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);

  diagnosticsEl.className = 'list';
  const item = document.createElement('div');
  item.className = 'diagnostic error';
  item.textContent = message;
  diagnosticsEl.replaceChildren(item);

  documentTitle.textContent = 'Unable to load document';
  documentBody.className = 'reader-body';
  documentBody.textContent = message;
}

function clickableRow(className: string) {
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

function updateActiveRows() {
  document.querySelectorAll<HTMLElement>('[data-folder]').forEach((row) => {
    row.classList.toggle('active', row.dataset.folder === selectedFolder);
  });

  document.querySelectorAll<HTMLElement>('[data-document]').forEach((row) => {
    row.classList.toggle('active', row.dataset.document === selectedDocument);
  });
}

function folderIcon(typeOrFolder: string): IconNode {
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

function normalizeFolderIconKey(typeOrFolder: string) {
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

function folderTitleFromResponse(id: string, metadata?: Record<string, unknown>) {
  const name = metadata?.name;
  return typeof name === 'string' && name.length > 0 ? name : titleFromId(id);
}

function depthFor(id: string) {
  return Math.max(0, id.split('/').length - 1);
}

function cssEscape(value: string) {
  return CSS.escape(value);
}

function titleFromId(id: string) {
  return titleFromSlug(id.split('/').at(-1) ?? id);
}

function titleFromSlug(slug: string) {
  return slug
    .split('-')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function formatLabel(label: string) {
  return label.replaceAll('_', ' ');
}

function renderPropertyValue(value: unknown) {
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

function renderPill(value: unknown) {
  const pill = document.createElement('span');
  pill.className = 'pill';
  pill.textContent = String(value);
  return pill;
}

function formatValue(value: unknown): string {
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

function requireElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) {
    throw new Error(`Missing element #${id}`);
  }
  return element as T;
}
