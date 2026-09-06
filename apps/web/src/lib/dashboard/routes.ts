//! What a URL can name, and how it is read and written.
//!
//! Addressing only. Nothing here selects anything or touches the panes — the
//! functions translate between the address bar and a `Route`, so the rules
//! about what may appear in a URL live in one place instead of being restated
//! at each call site.

import { foldersEl } from './elements';
import { cssEscape } from './format';

/// The query parameter naming a view that is not vault content.
///
/// A view is not a resource in the vault, so it is not addressed by a path.
/// That also settles the collision question by construction: a query string
/// cannot be mistaken for a canonical id, and no path has to be reserved.
export const VIEW_PARAM = 'view';
export const MODEL_VIEW = 'model';

/// What a URL can name.
///
/// A document or folder is addressed by its canonical id, so the path *is* the
/// id and a link reads as the thing it points at. A file takes its plain vault
/// path — the id grammar rules it out as a document, see `looksLikeId`. A view
/// is not vault content at all and is named by a query parameter, which no path
/// can collide with.
export type Route =
  { kind: 'id'; id: string } | { kind: 'file'; path: string } | { kind: 'model' } | null;

/// Whether `path` could be a canonical id.
///
/// `CanonicalId::parse` refuses any segment containing a dot and accepts only
/// lowercase letters, digits and hyphens, so anything else — an extension,
/// uppercase, a space, an underscore — is necessarily a file path rather than a
/// document. That is what lets both share the URL space without a prefix.
export function looksLikeId(path: string) {
  return /^[a-z0-9][a-z0-9-]*(\/[a-z0-9][a-z0-9-]*)*$/.test(path);
}

/// Whether `id` names a folder the tree is showing, used to decide if the
/// current history entry is a step in a descent rather than something the
/// reader chose to open.
export function isFolderRoute(id: string) {
  return foldersEl.querySelector(`[data-folder="${cssEscape(id)}"]`) !== null;
}

export function currentRoute(): Route {
  if (new URLSearchParams(window.location.search).get(VIEW_PARAM) === MODEL_VIEW) {
    return { kind: 'model' };
  }
  // Decoded per segment, mirroring `routePath`'s per-segment encode.
  // `decodeURI` deliberately leaves the reserved set (`+ & , # @ ;` and a
  // literal space) encoded, while `encodeURIComponent` encodes all of them — so
  // a file called `q1 & q2.pdf` came back as `q1 %26 q2.pdf` and its own deep
  // link 404'd. Ids never contain those characters; arbitrary file paths do.
  const raw = window.location.pathname
    .replace(/^\/+|\/+$/g, '')
    .split('/')
    .map(decodeURIComponent)
    .join('/');
  if (!raw) return null;
  // Id-shaped paths are resolved as documents first and fall back to files;
  // anything else cannot be an id at all. See `restoreRouteSelection`.
  return looksLikeId(raw) ? { kind: 'id', id: raw } : { kind: 'file', path: raw };
}

export function routePath(route: NonNullable<Route>) {
  const encode = (value: string) => value.split('/').map(encodeURIComponent).join('/');
  switch (route.kind) {
    case 'id':
      return `/${encode(route.id)}`;
    case 'file':
      return `/${encode(route.path)}`;
    case 'model':
      return `/?${VIEW_PARAM}=${MODEL_VIEW}`;
  }
}

/// Write a route to the address bar.
///
/// `replace` is for a view you can arrive at while looking for something else —
/// selecting a folder on the way down a tree. Those should be linkable and
/// survive a refresh without each one becoming a Back stop, or Back turns into
/// "collapse one level" instead of "the last thing I was reading".
export function setRoute(route: NonNullable<Route>, options: { replace?: boolean } = {}) {
  const nextPath = routePath(route);
  // Compare path *and* query: the model view differs from `/` only by query,
  // and a document route must clear a query left behind by it.
  if (window.location.pathname + window.location.search === nextPath) return;
  if (options.replace) {
    window.history.replaceState({}, '', nextPath);
  } else {
    window.history.pushState({}, '', nextPath);
  }
}

/// Every ancestor of `folder`, outermost first, including `folder` itself.
export function folderChain(folder: string) {
  const parts = folder.split('/').filter(Boolean);
  return parts.map((_, index) => parts.slice(0, index + 1).join('/'));
}
