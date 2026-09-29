import { beforeEach, expect, test } from 'bun:test';

import './test-dom';
import { installHistory } from './editing-test-support';

const { currentRoute, looksLikeId, routePath } = await import('./routes');

beforeEach(() => {
  installHistory();
});

for (const id of [
  'notes/lowercase-note',
  'notes/DE-Client-Producer-Infra-SOW8-260923',
  'Companies/Example/SOWs/Report-2026',
]) {
  test(`document route preserves canonical ID casing: ${id}`, () => {
    expect(looksLikeId(id)).toBe(true);
    window.location.pathname = routePath({ kind: 'id', id });
    expect(currentRoute()).toEqual({ kind: 'id', id });
  });
}

for (const path of ['notes/Report.md', 'assets/Plan.PDF', 'data/q1 & q2.json', 'notes/my_file']) {
  test(`non-ID file route still round-trips: ${path}`, () => {
    expect(looksLikeId(path)).toBe(false);
    window.location.pathname = routePath({ kind: 'file', path });
    expect(currentRoute()).toEqual({ kind: 'file', path });
  });
}

test('root and model routes are unchanged', () => {
  expect(currentRoute()).toBeNull();
  window.location.search = '?view=model';
  expect(currentRoute()).toEqual({ kind: 'model' });
});
