//! Searching the vault: running a query, and drawing what comes back.
//!
//! Opening a result is a navigation, and navigation lives in the selection
//! layer — so this module takes `SearchActions` instead of calling it. The
//! callbacks are coarse on purpose (`openDocument` expands the ancestors *and*
//! selects) so the navigation token that guards against interleaved clicks
//! stays on one side of the boundary rather than being passed across it.

import {
  getSearchStatus,
  searchVault,
  type SearchResponse,
  type SearchResult,
  type SearchStatus,
} from '../api';

import { clickableRow, emptyListNote, listSection } from './dom';
import { documentsEl, folderTitle, searchInput } from './elements';
import { basenameFromId } from './format';
import {
  appendSafeSnippet,
  renderMissingSearchIndex,
  renderSearchListError,
  renderSearchLoading,
  renderSearchStatus,
  searchMetaPill,
  searchSummary,
  setSearchStatusMessage,
} from './search-view';

/// What opening a result does, supplied by the selection layer.
export interface SearchActions {
  /// Put the list pane back to the folder that was showing before the query.
  restoreFolder: () => Promise<void>;
  openDocument: (id: string) => Promise<void>;
  openFolder: (id: string) => Promise<void>;
  run: (action: () => Promise<void>, options?: { owns?: 'document' }) => void;
}

let searchStatus: SearchStatus | null = null;
let searchDebounce: number | undefined;

/// Bumped by every query and by anything that supersedes one, so a slow
/// response that lands after the user has moved on is dropped rather than
/// replacing what they are now looking at.
let activeSearchRequest = 0;

/// Abandon whatever query is in flight. Called when a click elsewhere has
/// already decided what the list pane should show.
export function cancelPendingSearch() {
  activeSearchRequest += 1;
}

/// Debounced so a query runs on a pause in typing rather than per keystroke.
export function scheduleSearch(actions: SearchActions) {
  window.clearTimeout(searchDebounce);
  searchDebounce = window.setTimeout(() => {
    actions.run(() => runSearch(searchInput.value, actions));
  }, 180);
}

export async function refreshSearchStatus() {
  try {
    searchStatus = await getSearchStatus();
    renderSearchStatus(searchStatus);
  } catch (error) {
    searchStatus = null;
    const message = error instanceof Error ? error.message : String(error);
    setSearchStatusMessage(`Search unavailable: ${message}`);
  }
}

export async function runSearch(value: string, actions: SearchActions) {
  const query = value.trim();
  const requestId = ++activeSearchRequest;

  if (!query) {
    await actions.restoreFolder();
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
    renderSearchResults(response, actions);
  } catch (error) {
    if (requestId !== activeSearchRequest) return;
    renderSearchListError(error);
  }
}

export function renderSearchResults(response: SearchResponse, actions: SearchActions) {
  folderTitle.textContent = 'Search results';
  documentsEl.className = 'search-results-panel';

  const rows = response.results.map((result) => renderSearchResult(result, actions));
  const summary = searchSummary(response);
  const sections = [listSection(summary, rows.length > 0 ? rows : [emptyListNote('No results.')])];

  if (response.facets.length > 0) {
    sections.push(
      listSection('Result facets', [renderSearchFacetSummary(response.facets, actions)]),
    );
  }

  documentsEl.replaceChildren(...sections);
}

function renderSearchResult(result: SearchResult, actions: SearchActions) {
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
    actions.run(() => openSearchResult(result, actions), { owns: 'document' }),
  );
  return row;
}

function renderSearchFacetSummary(facets: SearchResponse['facets'], actions: SearchActions) {
  const wrapper = document.createElement('div');
  wrapper.className = 'search-facet-summary';
  wrapper.replaceChildren(
    ...facets.slice(0, 12).map(({ facet, count }) => {
      const pill = document.createElement('button');
      pill.className = 'pill search-facet-button';
      pill.type = 'button';
      pill.textContent = `${facet} ${count}`;
      pill.addEventListener('click', () => actions.run(() => runSearchWithFacet(facet, actions)));
      return pill;
    }),
  );
  return wrapper;
}

async function runSearchWithFacet(facet: string, actions: SearchActions) {
  const query = searchInput.value.trim();
  if (!query) return;
  renderSearchLoading(query);
  try {
    renderSearchResults(await searchVault({ q: query, facet, limit: 50 }), actions);
  } catch (error) {
    renderSearchListError(error);
  }
}

async function openSearchResult(result: SearchResult, actions: SearchActions) {
  searchInput.value = '';
  cancelPendingSearch();
  if (result.kind === 'document' && result.id) {
    await actions.openDocument(result.id);
    return;
  }

  if (result.kind === 'folder' && result.id) {
    await actions.openFolder(result.id);
  }
}
