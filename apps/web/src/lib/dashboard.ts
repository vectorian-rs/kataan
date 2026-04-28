import {
  getDocument,
  getFolder,
  getFolders,
  getVault,
  rebuildIndexes,
  validateVault,
  type Diagnostic,
  type DocumentResponse,
  type FolderDocument,
  type FolderSummary,
  type ValidateResponse,
} from './api';

const vaultSummary = requireElement<HTMLElement>('vault-summary');
const foldersEl = requireElement<HTMLElement>('folders');
const documentsEl = requireElement<HTMLElement>('documents');
const diagnosticsEl = requireElement<HTMLElement>('diagnostics');
const folderTitle = requireElement<HTMLElement>('folder-title');
const documentTitle = requireElement<HTMLElement>('document-title');
const documentBody = requireElement<HTMLElement>('document-body');
const metadataPanel = requireElement<HTMLElement>('metadata-panel');
const validateButton = requireElement<HTMLButtonElement>('validate-button');
const rebuildButton = requireElement<HTMLButtonElement>('rebuild-button');

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
});

async function loadVault() {
  const vault = await getVault();
  vaultSummary.textContent = `${vault.name} · schema ${vault.schema_version}`;
}

async function loadFolders() {
  const response = await getFolders();
  foldersEl.replaceChildren(...response.folders.map(renderFolderButton));
}

function renderFolderButton(folder: FolderSummary) {
  const button = document.createElement('button');
  button.className = 'item folder-row';
  button.type = 'button';

  const label = document.createElement('span');
  label.textContent = folder.name ?? folder.folder;

  const badge = document.createElement('span');
  badge.className = 'badge';
  badge.textContent = String(folder.document_count);

  button.append(label, badge);
  button.addEventListener('click', () => selectFolder(folder.folder));
  return button;
}

async function selectFolder(folder: string) {
  const response = await getFolder(folder);
  folderTitle.textContent = response.index.name;

  if (response.documents.length === 0) {
    documentsEl.className = 'list muted section';
    documentsEl.textContent = 'No documents.';
    return;
  }

  documentsEl.className = 'list';
  documentsEl.replaceChildren(...response.documents.map(renderDocumentButton));
}

function renderDocumentButton(vaultDocument: FolderDocument) {
  const button = document.createElement('button');
  button.className = 'item document-card';
  button.type = 'button';

  const title = document.createElement('strong');
  title.textContent = titleFromSlug(vaultDocument.slug);

  const meta = document.createElement('span');
  meta.className = 'muted';
  meta.textContent = vaultDocument.id;

  button.append(title, meta);
  button.addEventListener('click', () => selectDocument(vaultDocument.id));
  return button;
}

async function selectDocument(id: string) {
  const vaultDocument = await getDocument(id);
  documentTitle.textContent = titleFromId(vaultDocument.id);
  renderDocumentBody(vaultDocument);
  renderMetadata(vaultDocument);
}

function renderDocumentBody(vaultDocument: DocumentResponse) {
  documentBody.className = 'reader-body';
  documentBody.innerHTML = '';

  const markdown = document.createElement('pre');
  markdown.textContent = vaultDocument.markdown;
  documentBody.append(markdown);
}

function renderMetadata(vaultDocument: DocumentResponse) {
  metadataPanel.className = 'section metadata-grid';
  metadataPanel.replaceChildren(
    property('ID', vaultDocument.id),
    ...Object.entries(vaultDocument.metadata).map(([key, value]) => property(formatLabel(key), formatValue(value))),
  );
}

function property(label: string, value: unknown) {
  const wrapper = document.createElement('div');
  wrapper.className = 'property';

  const labelEl = document.createElement('div');
  labelEl.className = 'property-label';
  labelEl.textContent = label;

  const valueEl = document.createElement('div');
  valueEl.className = 'property-value';
  valueEl.textContent = formatValue(value);

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
  diagnosticsEl.className = 'list';
  const item = document.createElement('div');
  item.className = 'diagnostic error';
  item.textContent = error instanceof Error ? error.message : String(error);
  diagnosticsEl.replaceChildren(item);
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
