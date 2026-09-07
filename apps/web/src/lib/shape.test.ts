import { expect, test } from 'bun:test';

import { array, boolean, literals, mapOf, number, object, optional, string } from './shape';

const documentish = object({
  id: string,
  metadata: object({ type: string }),
  labels: array(string),
  count: number,
  ok: boolean,
  kind: literals('document', 'folder'),
  title: optional(string),
});

const valid = {
  id: 'notes/a',
  metadata: { type: 'note' },
  labels: ['x'],
  count: 1,
  ok: true,
  kind: 'document',
};

test('a well-formed response passes through', () => {
  expect(documentish(valid, 'root').id).toBe('notes/a');
});

test('a missing field names the field, not just the response', () => {
  const { id, ...without } = valid;
  expect(() => documentish(without, 'root')).toThrow('root.id: expected string, got undefined');
});

test('a wrong type names what it wanted and what it got', () => {
  expect(() => documentish({ ...valid, count: '1' }, 'root')).toThrow(
    'root.count: expected number, got string',
  );
});

test('a nested field reports its whole path', () => {
  expect(() => documentish({ ...valid, metadata: { type: 7 } }, 'root')).toThrow(
    'root.metadata.type: expected string, got number',
  );
});

test('an array reports the index that was wrong', () => {
  expect(() => documentish({ ...valid, labels: ['x', 2] }, 'root')).toThrow(
    'root.labels[1]: expected string, got number',
  );
});

test('a value outside a fixed set is refused', () => {
  expect(() => documentish({ ...valid, kind: 'file' }, 'root')).toThrow(
    'root.kind: expected "document" | "folder", got string',
  );
});

test('null and absent both satisfy optional, because Rust Option is either', () => {
  expect(documentish({ ...valid, title: null }, 'root').title).toBeUndefined();
  expect(documentish(valid, 'root').title).toBeUndefined();
  expect(documentish({ ...valid, title: 'T' }, 'root').title).toBe('T');
});

test('unknown keys are kept, so a server addition is not an outage', () => {
  // The one direction this is deliberately forgiving in: the server may add a
  // field before this app knows about it.
  const withExtra = documentish({ ...valid, added_later: 'fine' }, 'root');
  expect((withExtra as Record<string, unknown>).added_later).toBe('fine');
});

test('a map checks its values but not its key names', () => {
  const fields = mapOf(object({ type: string }));
  expect(Object.keys(fields({ email: { type: 'string' } }, 'root'))).toEqual(['email']);
  expect(() => fields({ email: { type: 1 } }, 'root')).toThrow(
    'root.email.type: expected string, got number',
  );
});

test('null is reported as null rather than object', () => {
  expect(() => string(null, 'root')).toThrow('root: expected string, got null');
});
