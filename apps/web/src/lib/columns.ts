//! The resizable columns, as data.
//!
//! Deliberately free of DOM and of anything the bundler has to resolve, because
//! two very different consumers need the same numbers:
//!
//! - `AppLayout.astro`'s inline script, which applies the saved widths *before*
//!   first paint. It cannot import a module — that is the whole point of it —
//!   so Astro serializes this into the page with `define:vars`.
//! - `dashboard/columns.ts`, which binds each column to its handle and does the
//!   dragging.
//!
//! They used to carry a copy each, matched by hand: the same three storage
//! keys, CSS properties and clamp ranges written twice in two languages. A
//! change to one clamp would have silently disagreed with the other, and the
//! disagreement would only show as a column that jumps on reload.

export type ResizableColumn = 'sidebar' | 'list' | 'properties';

export interface ColumnGeometry {
  key: ResizableColumn;
  cssProperty: string;
  storageKey: string;
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
  /// Which way the pointer moves to widen: the properties column is on the
  /// right, so dragging its handle left makes it bigger.
  dragDirection: 1 | -1;
}

export const COLUMN_GEOMETRY: ColumnGeometry[] = [
  {
    key: 'sidebar',
    cssProperty: '--sidebar-width',
    storageKey: 'kataan:sidebar-width',
    defaultWidth: 220,
    minWidth: 160,
    maxWidth: 420,
    dragDirection: 1,
  },
  {
    key: 'list',
    cssProperty: '--list-width',
    storageKey: 'kataan:list-width',
    defaultWidth: 320,
    minWidth: 220,
    maxWidth: 560,
    dragDirection: 1,
  },
  {
    key: 'properties',
    cssProperty: '--properties-width',
    storageKey: 'kataan:properties-width',
    defaultWidth: 260,
    minWidth: 220,
    maxWidth: 420,
    dragDirection: -1,
  },
];

export function clampColumnWidth(column: ColumnGeometry, width: number) {
  return Math.min(column.maxWidth, Math.max(column.minWidth, Math.round(width)));
}
