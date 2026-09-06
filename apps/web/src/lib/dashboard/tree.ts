//! The folder tree and the list pane: the rows, and which folders are open.
//!
//! Row builders are otherwise pure DOM construction — each differs from the
//! next only by what its click does. So the actions come in as `TreeActions`
//! rather than being called directly: this module renders and tracks what is
//! expanded, and the selection layer decides what opening something means.
//! Importing that layer back would make the two mutually dependent.

import { File } from 'lucide';
import { createElement } from 'lucide';

import type { FolderChild, FolderDocument, FolderFile, FolderSummary } from '../api';

import { clickableRow, emptyListNote, listSection } from './dom';
import { documentsEl, foldersEl } from './elements';
import { cssEscape, depthFor, fileExtensionClass } from './format';
import { folderIcon } from './icons';

/// What a row does when clicked. The selection layer supplies these once and
/// the tree never learns any more about it than this.
export interface TreeActions {
  openFolder: (folder: string) => void;
  openDocument: (id: string) => void;
  openFile: (file: FolderFile) => void;
}

/// Which folders are showing their children. Held here because it is only ever
/// true of a row this module drew, and only this module removes those rows.
const expandedFolderIds = new Set<string>();

export function isExpanded(folder: string) {
  return expandedFolderIds.has(folder);
}

export function renderFolderButton(folder: FolderSummary, actions: TreeActions) {
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
  button.addEventListener('click', () => actions.openFolder(folder.folder));
  return button;
}

export function renderChildFolders(parentId: string, folders: FolderChild[], actions: TreeActions) {
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
      row = renderChildFolderButton(folder, depthFor(folder.id), actions);
      insertAfter.after(row);
    }
    insertAfter = row;
  }
}

function renderChildFolderButton(folder: FolderChild, depth: number, actions: TreeActions) {
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
  button.addEventListener('click', () => actions.openFolder(folder.id));
  return button;
}

export function collapseFolder(folder: string) {
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

export function renderFolderContents(
  documents: FolderDocument[],
  files: FolderFile[],
  hasChildFolders: boolean,
  actions: TreeActions,
) {
  const children: HTMLElement[] = [];

  children.push(
    listSection(
      'Documents',
      documents.length > 0
        ? documents.map((entry) => renderDocumentButton(entry, actions))
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
      files.length > 0
        ? files.map((file) => renderFileRow(file, actions))
        : [emptyListNote('No files.')],
    ),
  );

  documentsEl.className = 'folder-contents';
  documentsEl.replaceChildren(...children);
}

function renderDocumentButton(vaultDocument: FolderDocument, actions: TreeActions) {
  const button = clickableRow('document-row');
  button.dataset.document = vaultDocument.id;

  const title = document.createElement('strong');
  title.textContent = vaultDocument.slug;

  const meta = document.createElement('span');
  meta.className = 'muted';
  meta.textContent = vaultDocument.id;

  button.append(title, meta);
  button.addEventListener('click', () => actions.openDocument(vaultDocument.id));
  return button;
}

function renderFileRow(file: FolderFile, actions: TreeActions) {
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
  row.addEventListener('click', () => actions.openFile(file));
  return row;
}
