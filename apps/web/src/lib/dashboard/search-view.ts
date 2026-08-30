import { emptyListNote, listSection } from './format';
import { type SearchResponse, type SearchStatus } from '../api';
import { documentsEl, folderTitle, searchStatusEl } from './elements';

export function renderSearchStatus(status: SearchStatus) {
  if (!status.exists || status.item_count === 0) {
    searchStatusEl.textContent = 'Search index not built. Use Rebuild.';
    return;
  }

  searchStatusEl.textContent = `Search ready · ${status.item_count} items`;
}

export function setSearchStatusMessage(message: string) {
  searchStatusEl.textContent = message;
}

export function renderMissingSearchIndex(query: string) {
  folderTitle.textContent = 'Search';
  documentsEl.className = 'search-results-panel';
  const message = emptyListNote(
    `Use Rebuild to build the search index before searching for “${query}”.`,
  );
  documentsEl.replaceChildren(listSection('Search index required', [message]));
}

export function renderSearchLoading(query: string) {
  folderTitle.textContent = 'Search';
  documentsEl.className = 'search-results-panel';
  documentsEl.replaceChildren(
    listSection('Searching', [emptyListNote(`Searching for “${query}”…`)]),
  );
}

export function renderSearchListError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  folderTitle.textContent = 'Search';
  documentsEl.className = 'search-results-panel';
  const note = document.createElement('div');
  note.className = 'diagnostic error';
  note.textContent = message;
  documentsEl.replaceChildren(listSection('Search error', [note]));
}

export function searchSummary(response: SearchResponse) {
  const count = response.results.length;
  const suffix = count === 1 ? 'result' : 'results';
  return `${count} ${suffix} for “${response.query}”`;
}

export function searchMetaPill(value: string) {
  const pill = document.createElement('span');
  pill.className = 'pill search-meta-pill';
  pill.textContent = value;
  return pill;
}

export function appendSafeSnippet(parent: HTMLElement, snippet: string) {
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
