import { File, PanelRightClose, PanelRightOpen, createElement } from 'lucide';

import {
  getDocument,
  getFile,
  getFolder,
  getHighlightedFile,
  getFolders,
  getSchema,
  getSearchStatus,
  getVault,
  rebuildIndexes,
  reindexSearch,
  resolvePath,
  searchVault,
  validateVault,
  type Diagnostic,
  type DocumentResponse,
  type FolderChild,
  type FolderDocument,
  type FolderFile,
  type FolderSummary,
  type SearchResponse,
  type SearchResult,
  type SearchStatus,
  type ValidateResponse,
} from './api';

import {
  RESIZABLE_COLUMNS,
  initColumnResizing,
  readSavedColumnWidth,
  setColumnWidth,
} from './dashboard/columns';
import { clickableRow, emptyListNote, listSection } from './dashboard/dom';
import {
  appShell,
  breadcrumb,
  diagnosticsEl,
  documentBody,
  documentTitle,
  documentsEl,
  folderTitle,
  foldersEl,
  metadataPanel,
  propertiesToggle,
  rebuildButton,
  searchForm,
  searchInput,
  validateButton,
  vaultSummary,
} from './dashboard/elements';
import { currentTheme, renderFileBody, renderHighlightedFile } from './dashboard/file-preview';
import {
  basenameFromId,
  cssEscape,
  depthFor,
  fileExtensionClass,
  folderTitleFromResponse,
  isHighlightableFile,
} from './dashboard/format';
import { folderIcon } from './dashboard/icons';
import { clearPanels, renderMetadata, renderSchema } from './dashboard/panels';
import {
  appendSafeSnippet,
  renderMissingSearchIndex,
  renderSearchListError,
  renderSearchLoading,
  renderSearchStatus,
  searchMetaPill,
  searchSummary,
  setSearchStatusMessage,
} from './dashboard/search-view';

/// Which navigation is current.
///
/// Selecting a folder, a document or a file awaits several fetches, so two
/// clicks in quick succession — or a click landing while a back-button restore
/// is still in flight — would otherwise interleave, and the pane would settle
/// on whichever request *finished* last rather than whichever was asked for
/// last. The row highlight is set synchronously, so the symptom is a reader
/// showing one document while a different row is marked active.
///
/// Every entry point takes a token and re-checks it after each await. A nested
/// call inherits its caller's token, so a folder selecting its first document
/// does not invalidate the folder load that started it.
let navigationGeneration = 0;

type Stale = () => boolean;

interface SelectOptions {
  selectFirst?: boolean;
  updateUrl?: boolean;
  stale?: Stale;
}

function beginNavigation(): Stale {
  const generation = ++navigationGeneration;
  return () => generation !== navigationGeneration;
}

let selectedFolder: string | null = null;
let selectedDocument: string | null = null;
let selectedFile: FolderFile | null = null;
let propertiesVisible = localStorage.getItem('kataan:properties-visible') === 'true';
let searchStatus: SearchStatus | null = null;
let searchDebounce: number | undefined;
let activeSearchRequest = 0;
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

// Internal links inside a rendered document carry the id the server resolved
// them to. Selecting in place keeps the app state; the anchor still has a real
// href, so middle-click and "copy link" behave normally.
documentBody.addEventListener('click', (event) => {
  const anchor = (event.target as HTMLElement | null)?.closest<HTMLAnchorElement>(
    'a[data-document]',
  );
  const id = anchor?.dataset.document;
  if (!id) return;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) {
    return;
  }
  event.preventDefault();
  void runAction(() => selectDocument(id), { owns: 'document' });
});

// Edge targets are re-rendered wholesale on every document, so the listener
// lives on the panel rather than on each button.
metadataPanel.addEventListener('click', (event) => {
  const target = (event.target as HTMLElement | null)?.closest<HTMLElement>('[data-edge]');
  const id = target?.dataset.edge;
  if (id) {
    void runAction(() => selectDocument(id), { owns: 'document' });
  }
});

// The URL is the whole of the app's navigation state, so back and forward are
// just "read the URL again". Without this the address bar moved and the view
// did not, because `pushState` alone does not re-render anything.
window.addEventListener('popstate', () => {
  void runAction(restoreRouteSelection, { owns: 'document' });
});

// Code blocks are highlighted server-side, so a theme switch has to re-fetch
// whatever is on screen — a document as much as a file preview.
window.addEventListener('kataan:theme-change', () => {
  if (selectedFile) {
    void runAction(() => selectFile(selectedFile as FolderFile), { owns: 'document' });
    return;
  }
  if (selectedDocument) {
    const id = selectedDocument;
    void runAction(() => selectDocument(id, { updateUrl: false }), { owns: 'document' });
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
    setSearchStatusMessage(`Search unavailable: ${message}`);
  }
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

function renderSearchResults(response: SearchResponse) {
  folderTitle.textContent = 'Search results';
  documentsEl.className = 'search-results-panel';

  const rows = response.results.map(renderSearchResult);
  const summary = searchSummary(response);
  const sections = [listSection(summary, rows.length > 0 ? rows : [emptyListNote('No results.')])];

  if (response.facets.length > 0) {
    sections.push(listSection('Result facets', [renderSearchFacetSummary(response.facets)]));
  }

  documentsEl.replaceChildren(...sections);
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
  row.addEventListener('click', () =>
    runAction(() => openSearchResult(result), { owns: 'document' }),
  );
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

async function openSearchResult(result: SearchResult) {
  searchInput.value = '';
  activeSearchRequest += 1;
  const stale = beginNavigation();
  if (result.kind === 'document' && result.id) {
    const folder = result.id.split('/').slice(0, -1).join('/');
    for (const chainFolder of folderChain(folder)) {
      await selectFolder(chainFolder, { selectFirst: false, stale });
      if (stale()) return;
    }
    await selectDocument(result.id, { stale });
    return;
  }

  if (result.kind === 'folder' && result.id) {
    for (const chainFolder of folderChain(result.id)) {
      await selectFolder(chainFolder, { selectFirst: false, stale });
      if (stale()) return;
    }
  }
}

function renderFolderButton(folder: FolderSummary) {
  const button = clickableRow('nav-row');
  button.dataset.folder = folder.folder;

  const label = document.createElement('span');
  label.className = 'folder-name';

  const icon = document.createElement('span');
  icon.className = `folder-icon ${folder.type}`;
  icon.append(
    createElement(folderIcon(folder.icon ?? folder.type), {
      width: 18,
      height: 18,
      'stroke-width': 2,
    }),
  );

  const name = document.createElement('span');
  name.textContent = folder.name ?? folder.folder;

  label.append(icon, name);

  const badge = document.createElement('span');
  badge.className = 'badge';
  badge.textContent = String(folder.document_count);

  button.append(label, badge);
  button.addEventListener('click', () =>
    runAction(() => handleFolderClick(folder.folder), { owns: 'document' }),
  );
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

async function selectFolder(folder: string, options: SelectOptions = {}) {
  const selectFirst = options.selectFirst ?? true;
  const stale = options.stale ?? beginNavigation();
  selectedFolder = folder;
  updateActiveRows();

  const response = await getFolder(folder);
  if (stale()) return;
  folderTitle.textContent = folderTitleFromResponse(response.id, response.metadata);
  renderChildFolders(folder, response.folders);

  renderFolderContents(response.documents, response.files, response.folders.length > 0);
  if (response.documents.length === 0) {
    selectedDocument = null;
    updateActiveRows();
    return;
  }
  if (selectFirst) {
    await selectDocument(response.documents[0].id, { stale });
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
  icon.append(
    createElement(folderIcon(folder.id.split('/')[0] ?? ''), {
      width: 18,
      height: 18,
      'stroke-width': 2,
    }),
  );

  const name = document.createElement('span');
  name.textContent = folder.name;

  label.append(icon, name);
  button.append(label);
  button.addEventListener('click', () =>
    runAction(() => handleFolderClick(folder.id), { owns: 'document' }),
  );
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

function renderFolderContents(
  documents: FolderDocument[],
  files: FolderFile[],
  hasChildFolders: boolean,
) {
  const children: HTMLElement[] = [];

  children.push(
    listSection(
      'Documents',
      documents.length > 0
        ? documents.map(renderDocumentButton)
        : [
            emptyListNote(
              hasChildFolders ? 'Select a nested folder or open a file.' : 'No documents.',
            ),
          ],
    ),
  );
  children.push(
    listSection(
      'Files',
      files.length > 0 ? files.map(renderFileRow) : [emptyListNote('No files.')],
    ),
  );

  documentsEl.className = 'folder-contents';
  documentsEl.replaceChildren(...children);
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
  button.addEventListener('click', () =>
    runAction(() => selectDocument(vaultDocument.id), { owns: 'document' }),
  );
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
  row.addEventListener('click', () => runAction(() => selectFile(file), { owns: 'document' }));
  return row;
}

async function selectFile(file: FolderFile, options: SelectOptions = {}) {
  const stale = options.stale ?? beginNavigation();
  selectedDocument = null;
  selectedFile = file;
  updateActiveRows();

  if (isHighlightableFile(file)) {
    try {
      const highlighted = await getHighlightedFile(file.path, currentTheme());
      if (stale()) return;
      breadcrumb.textContent = highlighted.path.replaceAll('/', ' › ');
      documentTitle.textContent = highlighted.name;
      renderHighlightedFile(highlighted.html);
      return;
    } catch {
      // Fall back to the generic file preview below if highlighting fails.
    }
  }

  const vaultFile = await getFile(file.path);
  if (stale()) return;
  breadcrumb.textContent = vaultFile.path.replaceAll('/', ' › ');
  documentTitle.textContent = vaultFile.name;
  renderFileBody(vaultFile);
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

async function selectDocument(id: string, options: SelectOptions = {}) {
  const updateUrl = options.updateUrl ?? true;
  const stale = options.stale ?? beginNavigation();
  selectedDocument = id;
  selectedFile = null;
  updateActiveRows();

  // The theme travels with the request: code blocks are highlighted server-side.
  const vaultDocument = await getDocument(id, currentTheme());
  if (stale()) return;
  breadcrumb.textContent = vaultDocument.id.replaceAll('/', ' › ');
  documentTitle.textContent = basenameFromId(vaultDocument.id);
  if (updateUrl) {
    updateRouteUrl(vaultDocument);
  }
  renderDocumentBody(vaultDocument);
  renderMetadata(vaultDocument);
  const schema = await getSchema('document');
  if (stale()) return;
  renderSchema(schema);
}

async function restoreRouteSelection() {
  const stale = beginNavigation();

  const locator = currentRouteLocator();
  if (!locator) {
    clearRouteSelection();
    return;
  }

  const resolved = await resolvePath(locator);
  if (stale()) return;

  if (resolved.is_folder_index) {
    await selectFolder(resolved.id, { selectFirst: false, stale });
    return;
  }

  for (const folder of folderChain(resolved.folder)) {
    await selectFolder(folder, { selectFirst: false, stale });
    if (stale()) return;
  }
  await selectDocument(resolved.id, { updateUrl: false, stale });
}

/// The view with nothing selected, as the page ships. Reached by going back
/// past the first document opened in this session.
function clearRouteSelection() {
  selectedDocument = null;
  selectedFile = null;
  breadcrumb.textContent = 'No document selected';
  documentTitle.textContent = 'Document';
  documentBody.className = 'reader-body empty-state';

  const card = document.createElement('div');
  card.className = 'empty-card';
  const heading = document.createElement('strong');
  heading.textContent = 'Select a document';
  card.append(heading, 'Choose a folder and document to preview its Markdown content.');
  documentBody.replaceChildren(card);

  clearPanels();
  updateActiveRows();
}

/// The path *is* the canonical id, so a URL can be read and shared.
function currentRouteLocator() {
  const id = decodeURI(window.location.pathname).replace(/^\/+|\/+$/g, '');
  if (!id) return null;
  // Ids are lowercase, digits, hyphens and slashes; anything else is not ours.
  if (!/^[a-z0-9][a-z0-9-]*(\/[a-z0-9][a-z0-9-]*)*$/.test(id)) return null;
  return id;
}

function updateRouteUrl(vaultDocument: DocumentResponse) {
  const nextPath = `/${vaultDocument.id.split('/').map(encodeURIComponent).join('/')}`;
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

/// Run an action, reporting a failure where the user was looking.
///
/// `owns: 'document'` marks the actions whose job *is* to put something in the
/// reader — selecting a document or a file. Only those may replace it on
/// failure. Everything else (validate, reindex, search, the theme toggle)
/// reports into the diagnostics list and leaves the open document alone;
/// previously a failed validate wiped the reader and mislabelled the failure as
/// "Unable to load document".
async function runAction(action: () => Promise<void>, options: { owns?: 'document' } = {}) {
  try {
    await action();
  } catch (error) {
    renderError(error, options.owns === 'document');
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

function renderError(error: unknown, replacesDocument = false) {
  const message = error instanceof Error ? error.message : String(error);

  diagnosticsEl.className = 'list';
  const item = document.createElement('div');
  item.className = 'diagnostic error';
  item.textContent = message;
  diagnosticsEl.replaceChildren(item);

  if (!replacesDocument) return;
  documentTitle.textContent = 'Unable to load document';
  documentBody.className = 'reader-body';
  documentBody.textContent = message;
}

function updateActiveRows() {
  document.querySelectorAll<HTMLElement>('[data-folder]').forEach((row) => {
    row.classList.toggle('active', row.dataset.folder === selectedFolder);
  });

  document.querySelectorAll<HTMLElement>('[data-document]').forEach((row) => {
    row.classList.toggle('active', row.dataset.document === selectedDocument);
  });
}
