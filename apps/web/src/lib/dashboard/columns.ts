import {
  appShell,
  listResizeHandle,
  propertiesResizeHandle,
  sidebarResizeHandle,
} from './elements';

export type ResizableColumn = 'sidebar' | 'list' | 'properties';

export type ColumnResizeConfig = {
  key: ResizableColumn;
  handle: HTMLElement;
  cssProperty: string;
  storageKey: string;
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
  dragDirection: 1 | -1;
};

export const COLUMN_KEYBOARD_STEP = 10;

export const RESIZABLE_COLUMNS: ColumnResizeConfig[] = [
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

export const columnWidths = new Map<ResizableColumn, number>();

export function initColumnResizing(column: ColumnResizeConfig) {
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

export function setColumnWidth(
  column: ColumnResizeConfig,
  width: number,
  options: { persist?: boolean } = {},
) {
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

export function getColumnWidth(column: ColumnResizeConfig) {
  return columnWidths.get(column.key) ?? column.defaultWidth;
}

export function readSavedColumnWidth(column: ColumnResizeConfig) {
  const savedWidth = Number.parseInt(localStorage.getItem(column.storageKey) ?? '', 10);
  return Number.isFinite(savedWidth) ? clampColumnWidth(column, savedWidth) : column.defaultWidth;
}

export function clampColumnWidth(column: ColumnResizeConfig, width: number) {
  return Math.min(column.maxWidth, Math.max(column.minWidth, Math.round(width)));
}
