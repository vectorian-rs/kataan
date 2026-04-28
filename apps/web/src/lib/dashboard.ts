import {
  getDocument,
  getFolder,
  getFolders,
  getVault,
  rebuildIndexes,
  validateVault,
  type Diagnostic,
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
  button.className = 'item';
  button.type = 'button';
  button.textContent = `${folder.name ?? folder.folder} (${folder.document_count})`;
  button.addEventListener('click', () => selectFolder(folder.folder));
  return button;
}

async function selectFolder(folder: string) {
  const response = await getFolder(folder);
  folderTitle.textContent = response.index.name;

  if (response.documents.length === 0) {
    documentsEl.className = 'list muted';
    documentsEl.textContent = 'No documents.';
    return;
  }

  documentsEl.className = 'list';
  documentsEl.replaceChildren(...response.documents.map(renderDocumentButton));
}

function renderDocumentButton(vaultDocument: FolderDocument) {
  const button = document.createElement('button');
  button.className = 'item';
  button.type = 'button';
  button.textContent = vaultDocument.id;
  button.addEventListener('click', () => selectDocument(vaultDocument.id));
  return button;
}

async function selectDocument(id: string) {
  const vaultDocument = await getDocument(id);
  documentTitle.textContent = vaultDocument.id;
  documentBody.innerHTML = '';

  const markdown = document.createElement('pre');
  markdown.textContent = vaultDocument.markdown;

  const metadata = document.createElement('pre');
  metadata.textContent = JSON.stringify(vaultDocument.metadata, null, 2);

  documentBody.append(markdown, metadata);
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
    diagnosticsEl.className = 'muted';
    diagnosticsEl.textContent = report.ok ? 'Vault is valid.' : 'No diagnostics.';
    return;
  }

  diagnosticsEl.className = '';
  diagnosticsEl.replaceChildren(...report.diagnostics.map(renderDiagnostic));
}

function renderDiagnostic(diagnostic: Diagnostic) {
  const item = document.createElement('div');
  item.className = `diagnostic ${diagnostic.severity}`;
  item.textContent = `${diagnostic.severity.toUpperCase()} [${diagnostic.code}] ${diagnostic.path ?? ''} ${diagnostic.message}`;
  return item;
}

function renderError(error: unknown) {
  diagnosticsEl.className = '';
  const item = document.createElement('div');
  item.className = 'diagnostic error';
  item.textContent = error instanceof Error ? error.message : String(error);
  diagnosticsEl.replaceChildren(item);
}

function requireElement<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) {
    throw new Error(`Missing element #${id}`);
  }
  return element as T;
}
