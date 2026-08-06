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
  PanelRightClose,
  PanelRightOpen,
  Presentation,
  ReceiptText,
  BookOpen,
  User,
  createElement,
  type IconNode,
} from 'lucide';

import {
  getDocument,
  getFile,
  getFolder,
  getHighlightedFile,
  getFolders,
  getRawFileUrl,
  getSchema,
  getSearchStatus,
  getVault,
  rebuildIndexes,
  reindexSearch,
  resolveRoute,
  searchVault,
  validateVault,
  type Diagnostic,
  type DocumentResponse,
  type FolderChild,
  type FolderDocument,
  type FileResponse,
  type FolderFile,
  type FolderSummary,
  type SearchResponse,
  type SearchResult,
  type SearchStatus,
  type TomlSchemaResponse,
  type ValidateResponse,
} from './api';

const appShell = requireElement<HTMLElement>('app-shell');
const sidebarResizeHandle = requireElement<HTMLElement>('sidebar-resize-handle');
const listResizeHandle = requireElement<HTMLElement>('list-resize-handle');
const propertiesResizeHandle = requireElement<HTMLElement>('properties-resize-handle');
const propertiesToggle = requireElement<HTMLButtonElement>('properties-toggle');
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
const searchForm = requireElement<HTMLFormElement>('search-form');
const searchInput = requireElement<HTMLInputElement>('search-input');
const searchStatusEl = requireElement<HTMLElement>('search-status');

type ResizableColumn = 'sidebar' | 'list' | 'properties';

type ColumnResizeConfig = {
  key: ResizableColumn;
  handle: HTMLElement;
  cssProperty: string;
  storageKey: string;
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
  dragDirection: 1 | -1;
};

const COLUMN_KEYBOARD_STEP = 10;
const RESIZABLE_COLUMNS: ColumnResizeConfig[] = [
  {
    key: 'sidebar',
    handle: sidebarResizeHandle,
    cssProperty: '--sidebar-width',
    storageKey: 'kataan:sidebar-width',
    defaultWidth: 220,
    minWidth: 160,
    maxWidth: 420,
    dragDirection: 1,
  },
  {
    key: 'list',
    handle: listResizeHandle,
    cssProperty: '--list-width',
    storageKey: 'kataan:list-width',
    defaultWidth: 320,
    minWidth: 220,
    maxWidth: 560,
    dragDirection: 1,
  },
  {
    key: 'properties',
    handle: propertiesResizeHandle,
    cssProperty: '--properties-width',
    storageKey: 'kataan:properties-width',
    defaultWidth: 260,
    minWidth: 220,
    maxWidth: 420,
    dragDirection: -1,
  },
];

let selectedFolder: string | null = null;
let selectedDocument: string | null = null;
let selectedFile: FolderFile | null = null;
let propertiesVisible = localStorage.getItem('kataan:properties-visible') === 'true';
let searchStatus: SearchStatus | null = null;
let searchDebounce: number | undefined;
let activeSearchRequest = 0;
const columnWidths = new Map<ResizableColumn, number>();
const expandedFolderIds = new Set<string>();

for (const column of RESIZABLE_COLUMNS) {
  setColumnWidth(column, readSavedColumnWidth(column), { persist: false });
  initColumnResizing(column);
}
setPropertiesVisible(propertiesVisible, { persist: false });

propertiesToggle.addEventListener('click', () => {
  setPropertiesVisible(!propertiesVisible, { persist: true });
});

validateButton.addEventListener('click', async () => {
  await runAction(async () => {
    renderDiagnostics(await validateVault());
  });
});

rebuildButton.addEventListener('click', async () => {
  await runAction(async () => {
    setSearchStatusMessage('Rebuilding vault and search index…');
    await rebuildIndexes();
    await loadFolders();
    await reindexSearch();
    await refreshSearchStatus();
    if (searchInput.value.trim()) {
      await runSearch(searchInput.value);
    }
    renderDiagnostics(await validateVault());
  });
});

searchForm.addEventListener('submit', (event) => {
  event.preventDefault();
  void runAction(() => runSearch(searchInput.value));
});

searchInput.addEventListener('input', () => {
  window.clearTimeout(searchDebounce);
  searchDebounce = window.setTimeout(() => {
    void runAction(() => runSearch(searchInput.value));
  }, 180);
});


window.addEventListener('kataan:theme-change', () => {
  if (selectedFile) {
    void runAction(() => selectFile(selectedFile as FolderFile));
  }
});

await runAction(async () => {
  await loadVault();
  await loadFolders();
  await refreshSearchStatus();
  await restoreRouteSelection();
});

async function loadVault() {
  const vault = await getVault();
  vaultSummary.replaceChildren();

  const product = document.createElement('span');
  product.className = 'vault-product';
  product.textContent = 'kataan:';

  const name = document.createElement('span');
  name.className = 'vault-name';
  name.textContent = vault.name;

  const titleLine = document.createElement('span');
  titleLine.className = 'vault-title-line';
  titleLine.append(product, name);

  const schema = document.createElement('span');
  schema.className = 'vault-schema';
  schema.textContent = `schema ${vault.schema_version}`;

  vaultSummary.append(titleLine, schema);
}

async function loadFolders() {
  const response = await getFolders();
  foldersEl.replaceChildren(...response.folders.map(renderFolderButton));

  const firstNonEmptyFolder = response.folders.find((folder) => folder.document_count > 0);
  if (!currentRouteLocator() && response.folders.length > 0) {
    await selectFolder(firstNonEmptyFolder?.folder ?? response.folders[0].folder);
  }
}

async function refreshSearchStatus() {
  try {
    searchStatus = await getSearchStatus();
    renderSearchStatus(searchStatus);
  } catch (error) {
    searchStatus = null;
    const message = error instanceof Error ? error.message : String(error);
    searchStatusEl.textContent = `Search unavailable: ${message}`;
  }
}

function renderSearchStatus(status: SearchStatus) {
  if (!status.exists || status.item_count === 0) {
    searchStatusEl.textContent = 'Search index not built. Use Rebuild.';
    return;
  }

  searchStatusEl.textContent = `Search ready · ${status.item_count} items`;
}

function setSearchStatusMessage(message: string) {
  searchStatusEl.textContent = message;
}

async function runSearch(value: string) {
  const query = value.trim();
  const requestId = ++activeSearchRequest;

  if (!query) {
    if (selectedFolder) {
      await selectFolder(selectedFolder, { selectFirst: false });
    }
    return;
  }

  if (!searchStatus) {
    await refreshSearchStatus();
  }

  if (!searchStatus?.exists || searchStatus.item_count === 0) {
    renderMissingSearchIndex(query);
    return;
  }

  renderSearchLoading(query);
  try {
    const response = await searchVault({ q: query, limit: 50 });
    if (requestId !== activeSearchRequest) return;
    renderSearchResults(response);
  } catch (error) {
    if (requestId !== activeSearchRequest) return;
    renderSearchListError(error);
  }
}

function renderMissingSearchIndex(query: string) {
  folderTitle.textContent = 'Search';
  documentsEl.className = 'search-results-panel';
  const message = emptyListNote(`Use Rebuild to build the search index before searching for “${query}”.`);
  documentsEl.replaceChildren(listSection('Search index required', [message]));
}

function renderSearchLoading(query: string) {
  folderTitle.textContent = 'Search';
  documentsEl.className = 'search-results-panel';
  documentsEl.replaceChildren(listSection('Searching', [emptyListNote(`Searching for “${query}”…`)]));
}

function renderSearchListError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  folderTitle.textContent = 'Search';
  documentsEl.className = 'search-results-panel';
  const note = document.createElement('div');
  note.className = 'diagnostic error';
  note.textContent = message;
  documentsEl.replaceChildren(listSection('Search error', [note]));
}

function renderSearchResults(response: SearchResponse) {
  folderTitle.textContent = 'Search results';
  documentsEl.className = 'search-results-panel';

  const rows = response.results.map(renderSearchResult);
  const summary = searchSummary(response);
  const sections = [
    listSection(summary, rows.length > 0 ? rows : [emptyListNote('No results.')]),
  ];

  if (response.facets.length > 0) {
    sections.push(listSection('Result facets', [renderSearchFacetSummary(response.facets)]));
  }

  documentsEl.replaceChildren(...sections);
}

function searchSummary(response: SearchResponse) {
  const count = response.results.length;
  const suffix = count === 1 ? 'result' : 'results';
  return `${count} ${suffix} for “${response.query}”`;
}

function renderSearchResult(result: SearchResult) {
  const row = clickableRow('search-result-row');
  if (result.kind === 'document' && result.id) row.dataset.document = result.id;
  if (result.kind === 'folder' && result.id) row.dataset.folder = result.id;

  const topLine = document.createElement('div');
  topLine.className = 'search-result-topline';

  const kind = document.createElement('span');
  kind.className = `search-kind search-kind-${result.kind}`;
  kind.textContent = result.kind;

  const title = document.createElement('strong');
  title.textContent = result.title ?? result.id ?? basenameFromId(result.path);

  topLine.append(kind, title);

  const path = document.createElement('span');
  path.className = 'search-result-path muted';
  path.textContent = result.path;

  const metadata = document.createElement('div');
  metadata.className = 'search-result-metadata';
  if (result.type) metadata.append(searchMetaPill(result.type));
  if (result.status) metadata.append(searchMetaPill(result.status));
  metadata.append(...result.facets.slice(0, 6).map(searchMetaPill));

  const snippet = document.createElement('p');
  snippet.className = 'search-result-snippet muted';
  if (result.snippet) {
    appendSafeSnippet(snippet, result.snippet);
  } else {
    snippet.textContent = 'No snippet available.';
  }

  row.append(topLine, path, metadata, snippet);
  row.addEventListener('click', () => runAction(() => openSearchResult(result)));
  return row;
}

function renderSearchFacetSummary(facets: SearchResponse['facets']) {
  const wrapper = document.createElement('div');
  wrapper.className = 'search-facet-summary';
  wrapper.replaceChildren(
    ...facets.slice(0, 12).map(({ facet, count }) => {
      const pill = document.createElement('button');
      pill.className = 'pill search-facet-button';
      pill.type = 'button';
      pill.textContent = `${facet} ${count}`;
      pill.addEventListener('click', () => runAction(() => runSearchWithFacet(facet)));
      return pill;
    }),
  );
  return wrapper;
}

async function runSearchWithFacet(facet: string) {
  const query = searchInput.value.trim();
  if (!query) return;
  renderSearchLoading(query);
  try {
    renderSearchResults(await searchVault({ q: query, facet, limit: 50 }));
  } catch (error) {
    renderSearchListError(error);
  }
}

function searchMetaPill(value: string) {
  const pill = document.createElement('span');
  pill.className = 'pill search-meta-pill';
  pill.textContent = value;
  return pill;
}

function appendSafeSnippet(parent: HTMLElement, snippet: string) {
  const parts = snippet.split(/(<mark>|<\/mark>)/);
  let mark: HTMLElement | null = null;
  for (const part of parts) {
    if (part === '<mark>') {
      mark = document.createElement('mark');
      parent.append(mark);
      continue;
    }
    if (part === '</mark>') {
      mark = null;
      continue;
    }
    if (part.length > 0) {
      (mark ?? parent).append(document.createTextNode(part));
    }
  }
}

async function openSearchResult(result: SearchResult) {
  searchInput.value = '';
  activeSearchRequest += 1;
  if (result.kind === 'document' && result.id) {
    const folder = result.id.split('/').slice(0, -1).join('/');
    for (const chainFolder of folderChain(folder)) {
      await selectFolder(chainFolder, { selectFirst: false });
    }
    await selectDocument(result.id);
    return;
  }

  if (result.kind === 'folder' && result.id) {
    for (const chainFolder of folderChain(result.id)) {
      await selectFolder(chainFolder, { selectFirst: false });
    }
    return;
  }

  if (result.kind === 'file') {
    await selectFile({
      name: basenameFromId(result.path),
      path: result.path,
      extension: result.extension,
    });
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
  button.addEventListener('click', () => runAction(() => handleFolderClick(folder.folder)));
  return button;
}

async function handleFolderClick(folder: string) {
  searchInput.value = '';
  activeSearchRequest += 1;
  if (selectedFolder === folder && expandedFolderIds.has(folder)) {
    collapseFolder(folder);
    return;
  }
  await selectFolder(folder);
}

async function selectFolder(folder: string, options: { selectFirst?: boolean } = {}) {
  const selectFirst = options.selectFirst ?? true;
  selectedFolder = folder;
  updateActiveRows();

  const response = await getFolder(folder);
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

  collapseFolder(parentId);
  if (folders.length > 0) {
    parentRow.classList.add('expanded');
    parentRow.setAttribute('aria-expanded', 'true');
    expandedFolderIds.add(parentId);
  }

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
  button.addEventListener('click', () => runAction(() => handleFolderClick(folder.id)));
  return button;
}

function collapseFolder(folder: string) {
  const row = foldersEl.querySelector<HTMLElement>(`[data-folder="${cssEscape(folder)}"]`);
  row?.classList.remove('expanded');
  row?.setAttribute('aria-expanded', 'false');

  const descendantPrefix = `${folder}/`;
  foldersEl.querySelectorAll<HTMLElement>('[data-folder]').forEach((candidate) => {
    const candidateFolder = candidate.dataset.folder;
    if (candidateFolder?.startsWith(descendantPrefix)) {
      candidate.remove();
    }
  });

  for (const expandedFolder of [...expandedFolderIds]) {
    if (expandedFolder === folder || expandedFolder.startsWith(descendantPrefix)) {
      expandedFolderIds.delete(expandedFolder);
    }
  }
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
  title.textContent = vaultDocument.slug;

  const meta = document.createElement('span');
  meta.className = 'muted';
  meta.textContent = vaultDocument.id;

  button.append(title, meta);
  button.addEventListener('click', () => runAction(() => selectDocument(vaultDocument.id)));
  return button;
}

function renderFileRow(file: FolderFile) {
  const row = clickableRow(`file-row ${fileExtensionClass(file.extension)}`);

  const title = document.createElement('strong');
  title.textContent = file.name;

  const meta = document.createElement('span');
  meta.className = 'muted file-meta';
  if (file.extension) {
    const extension = document.createElement('span');
    extension.className = 'file-extension-label';
    extension.textContent = file.extension.toUpperCase();

    const path = document.createElement('span');
    path.className = 'file-path';
    path.textContent = file.path;

    meta.append(extension, path);
  } else {
    meta.textContent = file.path;
  }

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

function fileExtensionClass(extension: string | undefined) {
  const normalized = extension?.toLowerCase().replace(/[^a-z0-9-]/g, '-');
  return normalized ? `file-ext-${normalized}` : 'file-ext-none';
}

async function selectFile(file: FolderFile) {
  selectedDocument = null;
  selectedFile = file;
  updateActiveRows();

  if (isHighlightableFile(file)) {
    try {
      const highlighted = await getHighlightedFile(file.path, currentTheme());
      breadcrumb.textContent = highlighted.path.replaceAll('/', ' › ');
      documentTitle.textContent = highlighted.name;
      renderHighlightedFile(highlighted.html);
      return;
    } catch {
      // Fall back to the generic file preview below if highlighting fails.
    }
  }

  const vaultFile = await getFile(file.path);
  breadcrumb.textContent = vaultFile.path.replaceAll('/', ' › ');
  documentTitle.textContent = vaultFile.name;
  renderFileBody(vaultFile);
}

function isHighlightableFile(file: FolderFile) {
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

function currentTheme() {
  return document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light';
}

function initColumnResizing(column: ColumnResizeConfig) {
  let dragStartX = 0;
  let dragStartWidth = getColumnWidth(column);
  let isDragging = false;

  const moveColumn = (event: PointerEvent) => {
    if (!isDragging) return;
    setColumnWidth(column, dragStartWidth + (event.clientX - dragStartX) * column.dragDirection);
  };

  const stopDragging = () => {
    if (!isDragging) return;
    isDragging = false;
    document.body.classList.remove('column-resizing');
    column.handle.classList.remove('is-resizing');
    window.removeEventListener('pointermove', moveColumn);
    window.removeEventListener('pointerup', stopDragging);
    window.removeEventListener('pointercancel', stopDragging);
  };

  column.handle.addEventListener('pointerdown', (event) => {
    if (event.button !== 0) return;
    event.preventDefault();
    isDragging = true;
    dragStartX = event.clientX;
    dragStartWidth = getColumnWidth(column);
    document.body.classList.add('column-resizing');
    column.handle.classList.add('is-resizing');
    column.handle.setPointerCapture?.(event.pointerId);
    window.addEventListener('pointermove', moveColumn);
    window.addEventListener('pointerup', stopDragging);
    window.addEventListener('pointercancel', stopDragging);
  });

  column.handle.addEventListener('dblclick', () => {
    setColumnWidth(column, column.defaultWidth);
  });

  column.handle.addEventListener('keydown', (event) => {
    const step = event.shiftKey ? COLUMN_KEYBOARD_STEP * 4 : COLUMN_KEYBOARD_STEP;
    if (event.key === 'ArrowLeft') {
      event.preventDefault();
      setColumnWidth(column, getColumnWidth(column) - step * column.dragDirection);
    }
    if (event.key === 'ArrowRight') {
      event.preventDefault();
      setColumnWidth(column, getColumnWidth(column) + step * column.dragDirection);
    }
    if (event.key === 'Home') {
      event.preventDefault();
      setColumnWidth(column, column.minWidth);
    }
    if (event.key === 'End') {
      event.preventDefault();
      setColumnWidth(column, column.maxWidth);
    }
  });
}

function setColumnWidth(column: ColumnResizeConfig, width: number, options: { persist?: boolean } = {}) {
  const nextWidth = clampColumnWidth(column, width);
  columnWidths.set(column.key, nextWidth);
  appShell.style.setProperty(column.cssProperty, `${nextWidth}px`);
  column.handle.setAttribute('aria-valuemin', String(column.minWidth));
  column.handle.setAttribute('aria-valuemax', String(column.maxWidth));
  column.handle.setAttribute('aria-valuenow', String(nextWidth));
  if (options.persist ?? true) {
    localStorage.setItem(column.storageKey, String(nextWidth));
  }
}

function getColumnWidth(column: ColumnResizeConfig) {
  return columnWidths.get(column.key) ?? column.defaultWidth;
}

function readSavedColumnWidth(column: ColumnResizeConfig) {
  const savedWidth = Number.parseInt(localStorage.getItem(column.storageKey) ?? '', 10);
  return Number.isFinite(savedWidth) ? clampColumnWidth(column, savedWidth) : column.defaultWidth;
}

function clampColumnWidth(column: ColumnResizeConfig, width: number) {
  return Math.min(column.maxWidth, Math.max(column.minWidth, Math.round(width)));
}

function setPropertiesVisible(visible: boolean, options: { persist?: boolean } = {}) {
  propertiesVisible = visible;
  appShell.classList.toggle('properties-hidden', !visible);
  propertiesToggle.setAttribute('aria-pressed', String(visible));
  propertiesToggle.setAttribute(
    'aria-label',
    visible ? 'Hide properties sidebar' : 'Open properties sidebar',
  );
  propertiesToggle.classList.toggle('button-primary', !visible);
  propertiesToggle.classList.toggle('button-secondary', visible);
  const icon = createElement(visible ? PanelRightClose : PanelRightOpen, {
    width: 16,
    height: 16,
    'stroke-width': 2,
  });
  const label = document.createElement('span');
  label.textContent = 'Sidebar';
  propertiesToggle.replaceChildren(icon, label);
  if (options.persist ?? true) {
    localStorage.setItem('kataan:properties-visible', String(visible));
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

  if (file.kind === 'html') {
    documentBody.className = 'reader-body file-reader-body html-reader-body';
    const frame = document.createElement('iframe');
    frame.className = 'html-preview';
    frame.title = file.name;
    frame.setAttribute('sandbox', 'allow-scripts');
    frame.srcdoc = file.content;
    documentBody.append(frame);
    return;
  }

  if (file.kind === 'text') {
    const pre = document.createElement('pre');
    pre.className = 'file-preview';
    pre.textContent = file.content;
    documentBody.append(pre);
    return;
  }

  if (file.kind === 'pdf') {
    documentBody.className = 'reader-body file-reader-body pdf-reader-body';
    const frame = document.createElement('iframe');
    frame.className = 'pdf-preview';
    frame.title = file.name;
    frame.src = getRawFileUrl(file.path);
    documentBody.append(frame);
    return;
  }

  if (file.kind === 'image') {
    documentBody.className = 'reader-body file-reader-body image-reader-body';
    const preview = document.createElement('img');
    preview.className = 'image-preview';
    preview.src = getRawFileUrl(file.path);
    preview.alt = file.name;
    documentBody.append(preview);
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
  selectedFile = null;
  updateActiveRows();

  const vaultDocument = await getDocument(id);
  breadcrumb.textContent = vaultDocument.id.replaceAll('/', ' › ');
  documentTitle.textContent = basenameFromId(vaultDocument.id);
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
  documentBody.innerHTML = vaultDocument.html;
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
  return typeof name === 'string' && name.length > 0 ? name : basenameFromId(id);
}

function depthFor(id: string) {
  return Math.max(0, id.split('/').length - 1);
}

function cssEscape(value: string) {
  return CSS.escape(value);
}

function basenameFromId(id: string) {
  return id.split('/').at(-1) ?? id;
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
