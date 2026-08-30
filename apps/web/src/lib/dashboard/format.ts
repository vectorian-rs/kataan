//! Pure string and predicate helpers. No DOM, no icons.

import { type FolderFile } from '../api';

export function cssEscape(value: string) {
  return CSS.escape(value);
}

export function basenameFromId(id: string) {
  return id.split('/').at(-1) ?? id;
}

export function formatLabel(label: string) {
  return label.replaceAll('_', ' ');
}

export function formatValue(value: unknown): string {
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

export function depthFor(id: string) {
  return Math.max(0, id.split('/').length - 1);
}

export function folderTitleFromResponse(id: string, metadata?: Record<string, unknown>) {
  const name = metadata?.name;
  return typeof name === 'string' && name.length > 0 ? name : basenameFromId(id);
}

export function fileExtensionClass(extension: string | undefined) {
  const normalized = extension?.toLowerCase().replace(/[^a-z0-9-]/g, '-');
  return normalized ? `file-ext-${normalized}` : 'file-ext-none';
}

export function isHighlightableFile(file: FolderFile) {
  return new Set([
    'bash',
    'c',
    'cc',
    'cpp',
    'cxx',
    'h',
    'hpp',
    'hs',
    'hxx',
    'js',
    'json',
    'md',
    'py',
    'rs',
    'sh',
    'toml',
    'ts',
    'txt',
    'yaml',
    'yml',
  ]).has(file.extension?.toLowerCase() ?? '');
}
