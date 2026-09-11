import {
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  createElement,
} from 'lucide';

import {
  type CanonicalFolderResponse,
  getDocument,
  getOntology,
  getFile,
  getFolder,
  getHighlightedFile,
  getFolders,
  getSchema,
  getVault,
  rebuildIndexes,
  reindexSearch,
  resolvePath,
  validateVault,
  type Diagnostic,
  type DocumentResponse,
  type TomlSchemaResponse,
  type FolderFile,
  type ValidateResponse,
} from './api';

import {
  RESIZABLE_COLUMNS,
  initColumnResizing,
  readSavedColumnWidth,
  setColumnWidth,
} from './dashboard/columns';
import {
  appShell,
  breadcrumb,
  diagnosticsEl,
  cancelButton,
  documentBody,
  documentEditor,
  documentTitle,
  folderTitle,
  foldersEl,
  listToggle,
  editButton,
  metadataPanel,
  ontologyButton,
  saveButton,
  propertiesToggle,
  rebuildButton,
  searchForm,
  searchInput,
  validateButton,
  vaultSummary,
} from './dashboard/elements';
import { currentTheme, renderFileBody, renderHighlightedFile } from './dashboard/file-preview';
import { basenameFromId, folderTitleFromResponse, isHighlightableFile } from './dashboard/format';
import { clearPanels, renderMetadata, renderSchema } from './dashboard/panels';
import { renderOntology } from './dashboard/ontology-view';
import {
  cancelPendingSearch,
  refreshSearchStatus,
  runSearch,
  scheduleSearch,
  type SearchActions,
} from './dashboard/search';
import {
  collapseFolder,
  isExpanded,
  renderChildFolders,
  renderFolderButton,
  renderFolderContents,
  type TreeActions,
} from './dashboard/tree';
import {
  beginEditing,
  cancelEditing,
  isEditing,
  saveEditing,
  setOpenDocument,
} from './dashboard/editing';
import { currentRoute, folderChain, isFolderRoute, setRoute } from './dashboard/routes';
import { setSearchStatusMessage } from './dashboard/search-view';

/// One panel toggle, configured twice.
///
/// The list and properties panels do the same job on opposite sides of the
/// reader; writing them separately meant nineteen character-identical lines and
/// two chances for their a11y or button styling to drift apart.
///
/// Declared above the boot block for the same reason the route constants are:
/// a module-level `const` is not hoisted, and boot reads these.
interface PanelToggle {
  button: HTMLButtonElement;
  /// Owned by `setPanelVisible`, so the flag and the DOM cannot disagree.
  visible: boolean;
  /// Class the shell carries while the panel is hidden.
  shellClass: string;
  storageKey: string;
  label: string;
  /// Icons for the visible and hidden states.
  icons: [typeof PanelLeftClose, typeof PanelLeftOpen];
  /// Aria labels for the visible (hide) and hidden (show) states.
  aria: [string, string];
}

const LIST_TOGGLE: PanelToggle = {
  button: listToggle,
  visible: localStorage.getItem('kataan:list-visible') !== 'false',
  shellClass: 'list-hidden',
  storageKey: 'kataan:list-visible',
  label: 'List',
  icons: [PanelLeftClose, PanelLeftOpen],
  aria: ['Hide document list', 'Show document list'],
};

const PROPERTIES_TOGGLE: PanelToggle = {
  button: propertiesToggle,
  visible: localStorage.getItem('kataan:properties-visible') === 'true',
  shellClass: 'properties-hidden',
  storageKey: 'kataan:properties-visible',
  label: 'Sidebar',
  icons: [PanelRightClose, PanelRightOpen],
  aria: ['Hide properties sidebar', 'Open properties sidebar'],
};

/// One fetch per type, kept because a type's schema changes only when the
/// ontology does — and `forgetDocumentSchema` clears these alongside it.
///
/// Declared above the boot block, like every other module-level `const` here:
/// boot reaches this through `selectDocument`, and a `const` initialised later
/// in the module is still `undefined` at that point. The bundler lowers it to
/// `var`, so the failure is a `TypeError` at run time rather than anything the
/// type checker or a temporal-dead-zone error would catch.
const typeSchemas = new Map<string, Promise<TomlSchemaResponse>>();

function typeSchemaFor(vaultDocument: DocumentResponse) {
  const type = String(vaultDocument.metadata.type ?? '');
  let request = typeSchemas.get(type);
  if (!request) {
    request = getSchema(type);
    typeSchemas.set(type, request);
  }
  return request;
}

/// `/api/schema/document` describes kataan's own metadata struct plus the
/// vault's constraints — the same ~1.7 KB for every document, and constant
/// until the vault reloads. Fetched once instead of on every selection.
let documentSchemaRequest: ReturnType<typeof getSchema> | undefined;

function documentSchema() {
  documentSchemaRequest ??= getSchema('document');
  return documentSchemaRequest;
}

/// Called after anything that can change the vault's constraints, so the next
/// selection re-fetches rather than rendering a schema from before the change.
function forgetDocumentSchema() {
  documentSchemaRequest = undefined;
  typeSchemas.clear();
}

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

/// What clicking a row in the tree does. Declared above the boot block, like
/// every other module-level `const` here — the bundler lowers `const` to `var`,
/// so one initialised further down is `undefined` when boot reads it, with no
/// temporal-dead-zone error to notice.
/// What opening a search result does. The navigation token is created here, so
/// the chain of ancestors and the document itself belong to one navigation and
/// a second click supersedes the whole of it rather than half.
const searchActions: SearchActions = {
  restoreFolder: async () => {
    if (selectedFolder) await selectFolder(selectedFolder, { selectFirst: false });
  },
  openDocument: async (id) => {
    const stale = beginNavigation();
    await expandChain(id.split('/').slice(0, -1).join('/'), stale);
    if (stale()) return;
    await selectDocument(id, { stale });
  },
  openFolder: async (id) => {
    const stale = beginNavigation();
    await expandChain(id, stale);
  },
  run: (action, options) => void runAction(action, options),
};

const treeActions: TreeActions = {
  openFolder: (folder) => void runAction(() => handleFolderClick(folder), { owns: 'document' }),
  openDocument: (id) => void runAction(() => selectDocument(id), { owns: 'document' }),
  openFile: (file) => void runAction(() => selectFile(file), { owns: 'document' }),
};

let selectedFolder: string | null = null;
let selectedDocument: string | null = null;
let selectedFile: FolderFile | null = null;

for (const column of RESIZABLE_COLUMNS) {
  setColumnWidth(column, readSavedColumnWidth(column), { persist: false });
  initColumnResizing(column);
}
setPanelVisible(PROPERTIES_TOGGLE, PROPERTIES_TOGGLE.visible, { persist: false });
setPanelVisible(LIST_TOGGLE, LIST_TOGGLE.visible, { persist: false });

propertiesToggle.addEventListener('click', () => {
  setPanelVisible(PROPERTIES_TOGGLE, !PROPERTIES_TOGGLE.visible, { persist: true });
});

editButton.addEventListener('click', beginEditing);
cancelButton.addEventListener('click', cancelEditing);
/// Save, then re-read. Shared by the button and Cmd/Ctrl+S so the two cannot
/// drift into doing different things.
function save() {
  void runAction(() => saveEditing((id) => selectDocument(id, { updateUrl: false })), {
    owns: 'document',
  });
}

saveButton.addEventListener('click', save);

// Cmd/Ctrl+S saves, Escape cancels — a textarea that only commits by mouse is
// not an editor anyone will use.
documentEditor.addEventListener('keydown', (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key === 's') {
    event.preventDefault();
    save();
  } else if (event.key === 'Escape') {
    event.preventDefault();
    cancelEditing();
  }
});

listToggle.addEventListener('click', () => {
  setPanelVisible(LIST_TOGGLE, !LIST_TOGGLE.visible, { persist: true });
});

// The model, not the data: what types exist and what may link to what. Read
// only — it is generated from ontology.toml and the type registry.
ontologyButton.addEventListener('click', () => {
  void runAction(
    async () => {
      const stale = beginNavigation();
      selectedDocument = null;
      selectedFile = null;
      updateActiveRows();
      const ontology = await getOntology();
      if (stale()) return;
      renderOntology(ontology);
      clearPanels();
      setRoute({ kind: 'model' });
    },
    { owns: 'document' },
  );
});

validateButton.addEventListener('click', async () => {
  await runAction(async () => {
    forgetDocumentSchema();
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
      await runSearch(searchInput.value, searchActions);
    }
    forgetDocumentSchema();
    renderDiagnostics(await validateVault());
  });
});

searchForm.addEventListener('submit', (event) => {
  event.preventDefault();
  void runAction(() => runSearch(searchInput.value, searchActions));
});

searchInput.addEventListener('input', () => scheduleSearch(searchActions));

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
  // Re-selecting rebuilds the reader from disk, which discards an open draft.
  // The theme only changes server-rendered syntax highlighting, and that is in
  // the preview — hidden while editing. Leaving it stale until the next load
  // costs nothing; losing someone's unsaved text costs everything.
  if (isEditing()) return;
  if (selectedFile) {
    void runAction(() => selectFile(selectedFile as FolderFile), { owns: 'document' });
    return;
  }
  if (selectedDocument) {
    const id = selectedDocument;
    void runAction(() => selectDocument(id, { updateUrl: false }), { owns: 'document' });
  }
});

// Boot owns the reader: it is restoring whatever the URL names, so a deep link
// that cannot be resolved should explain itself there rather than leaving the
// shipped "Select a document" card up with the reason buried in diagnostics.
await runAction(
  async () => {
    await loadVault();
    await loadFolders();
    await refreshSearchStatus();
    await restoreRouteSelection();
  },
  { owns: 'document' },
);

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
  foldersEl.replaceChildren(
    ...response.folders.map((folder) => renderFolderButton(folder, treeActions)),
  );

  const firstNonEmptyFolder = response.folders.find((folder) => folder.document_count > 0);
  if (!currentRoute() && response.folders.length > 0) {
    await selectFolder(firstNonEmptyFolder?.folder ?? response.folders[0].folder);
  }
}

async function handleFolderClick(folder: string) {
  searchInput.value = '';
  cancelPendingSearch();
  if (selectedFolder === folder && isExpanded(folder)) {
    collapseFolder(folder);
    return;
  }
  await selectFolder(folder);
}

async function selectFolder(folder: string, options: SelectOptions = {}) {
  const stale = options.stale ?? beginNavigation();
  selectedFolder = folder;
  updateActiveRows();

  const response = await getFolder(folder);
  if (stale()) return;
  // Replace only when the entry being overwritten is itself a folder — one
  // step of the same descent. Replacing unconditionally overwrote whatever the
  // reader had open: from `/notes/alpha`, clicking a folder replaced that entry
  // and then pushed the folder's first document, so Back never returned to
  // `notes/alpha` at all. That is precisely what the replace was meant to
  // protect.
  if (options.updateUrl ?? true) {
    const replacing = currentRoute();
    const replace = replacing?.kind === 'id' && isFolderRoute(replacing.id);
    setRoute({ kind: 'id', id: folder }, { replace });
  }
  const first = applyFolder(folder, response, options);
  if (first) {
    await selectDocument(first, { stale });
  }
}

/// Render a folder's contents. Separated from fetching it so a chain of
/// ancestors can be fetched together and rendered in order.
///
/// Returns the document to open next, when the caller asked for the folder's
/// first — the fetch and the render are synchronous, so the await stays with
/// the caller.
function applyFolder(
  folder: string,
  response: CanonicalFolderResponse,
  options: SelectOptions,
): string | undefined {
  folderTitle.textContent = folderTitleFromResponse(response.id, response.metadata);
  renderChildFolders(folder, response.folders, treeActions);
  renderFolderContents(
    response.documents,
    response.files,
    response.folders.length > 0,
    treeActions,
  );

  if (response.documents.length === 0) {
    selectedDocument = null;
    updateActiveRows();
    return undefined;
  }
  return (options.selectFirst ?? true) ? response.documents[0].id : undefined;
}

async function selectFile(file: FolderFile, options: SelectOptions = {}) {
  const stale = options.stale ?? beginNavigation();
  selectedDocument = null;
  setOpenDocument(null);
  selectedFile = file;
  updateActiveRows();
  if (options.updateUrl ?? true) {
    setRoute({ kind: 'file', path: file.path });
  }

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

function setPanelVisible(
  panel: PanelToggle,
  visible: boolean,
  options: { persist?: boolean } = {},
) {
  panel.visible = visible;
  panel.button.setAttribute('aria-pressed', String(visible));
  panel.button.setAttribute('aria-label', visible ? panel.aria[0] : panel.aria[1]);
  panel.button.classList.toggle('button-primary', !visible);
  panel.button.classList.toggle('button-secondary', visible);
  appShell.classList.toggle(panel.shellClass, !visible);

  const icon = createElement(visible ? panel.icons[0] : panel.icons[1], {
    width: 16,
    height: 16,
    'stroke-width': 2,
  });
  const label = document.createElement('span');
  label.textContent = panel.label;
  panel.button.replaceChildren(icon, label);

  if (options.persist ?? true) {
    localStorage.setItem(panel.storageKey, String(visible));
  }
}

async function selectDocument(id: string, options: SelectOptions = {}) {
  const updateUrl = options.updateUrl ?? true;
  const stale = options.stale ?? beginNavigation();
  selectedDocument = id;
  selectedFile = null;
  updateActiveRows();

  // The theme travels with the request: code blocks are highlighted
  // server-side. The schema is memoized, so this is one round trip in practice.
  const [vaultDocument, schema] = await Promise.all([
    getDocument(id, currentTheme()),
    documentSchema(),
  ]);
  if (stale()) return;
  // The *type's* schema carries `node_schema`; the generic `document` one above
  // describes kataan's own keys and is what the schema panel shows. Absent when
  // the type declares nothing, which the form handles.
  const typeSchema = await typeSchemaFor(vaultDocument).catch(() => undefined);
  if (stale()) return;
  breadcrumb.textContent = vaultDocument.id.replaceAll('/', ' › ');
  documentTitle.textContent = basenameFromId(vaultDocument.id);
  if (updateUrl) {
    updateRouteUrl(vaultDocument);
  }
  setOpenDocument({
    id: vaultDocument.id,
    markdown: vaultDocument.markdown,
    updatedAt:
      typeof vaultDocument.metadata.updated_at === 'string'
        ? vaultDocument.metadata.updated_at
        : undefined,
    document: vaultDocument,
    schema: typeSchema,
  });
  renderDocumentBody(vaultDocument);
  renderMetadata(vaultDocument);
  renderSchema(schema);
}

async function restoreRouteSelection() {
  const stale = beginNavigation();

  const route = currentRoute();
  if (!route) {
    clearRouteSelection();
    return;
  }

  if (route.kind === 'model') {
    const ontology = await getOntology();
    if (stale()) return;
    renderOntology(ontology);
    clearPanels();
    return;
  }

  if (route.kind === 'file') {
    await selectFileByPath(route.path, stale);
    return;
  }

  // A document wins over a file of the same name. Ids are the vault's primary
  // namespace, and a file whose path is also id-shaped (`docs/readme`, no
  // extension) is the only case where both could match.
  const resolved = await resolvePath(route.id).catch(() => null);
  if (stale()) return;
  if (!resolved) {
    // Not a document. It may still be a file whose path happens to be
    // id-shaped (`docs/readme`, no extension). If it is neither, say that
    // rather than reporting whichever lookup happened to run last — the reader
    // typed one path and does not care which namespace we tried second.
    try {
      await selectFileByPath(route.id, stale);
    } catch {
      throw new Error(`\`${route.id}\` is not a document or a file in this vault`);
    }
    return;
  }

  if (resolved.is_folder_index) {
    await selectFolder(resolved.id, { selectFirst: false, stale, updateUrl: false });
    if (stale()) return;
    // A folder index is a document — it has its own `index.md`. Render it,
    // rather than expanding the tree and leaving whatever was open before in
    // the reader: now that folders have URLs, `/people` has to show something
    // that is actually `people`.
    await selectDocument(resolved.id, { updateUrl: false, stale });
    return;
  }

  await expandChain(resolved.folder, stale);
  if (stale()) return;
  await selectDocument(resolved.id, { updateUrl: false, stale });
}

/// Expand every ancestor of `folder` so the target document's row exists.
///
/// The requests are independent — each ancestor's children come from its own
/// `/api/folder` — so they go out together. Awaiting them one at a time made
/// opening a five-deep document from a URL five serial round trips before the
/// document itself was even requested.
async function expandChain(folder: string, stale: Stale) {
  const chain = folderChain(folder);
  const responses = await Promise.all(chain.map((ancestor) => getFolder(ancestor)));
  if (stale()) return;
  // Rendered in chain order: each ancestor's row must exist before its children
  // are inserted beneath it.
  for (const [index, ancestor] of chain.entries()) {
    applyFolder(ancestor, responses[index], { selectFirst: false });
  }
}

/// Open a file preview from a path alone.
///
/// Only the path survives in a URL, so the rest of `FolderFile` is derived from
/// it — `selectFile` needs the extension to decide whether the preview can be
/// syntax-highlighted.
async function selectFileByPath(path: string, stale: Stale) {
  const name = path.split('/').pop() ?? path;
  const dot = name.lastIndexOf('.');
  await selectFile(
    { name, path, extension: dot > 0 ? name.slice(dot + 1) : undefined },
    { stale, updateUrl: false },
  );
}

/// The view with nothing selected, as the page ships. Reached by going back
/// past the first document opened in this session.
function clearRouteSelection() {
  selectedDocument = null;
  setOpenDocument(null);
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

function updateRouteUrl(vaultDocument: DocumentResponse) {
  setRoute({ kind: 'id', id: vaultDocument.id });
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
