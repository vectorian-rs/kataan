import { expect, test } from 'bun:test';

import { display, listRoundTrips, parse, unchanged } from './metadata-values';

test('a boolean shows as true/false and comes back as a boolean', () => {
  // It displayed as '' and read back as null, so an untouched `enabled = true`
  // was deleted by a save that only changed the Markdown.
  expect(display(true)).toBe('true');
  expect(display(false)).toBe('false');
  expect(parse('true', true, 'boolean')).toBe(true);
  expect(parse('false', true, 'boolean')).toBe(false);
  expect(unchanged(parse(display(true), true, 'boolean'), true)).toBe(true);
});

test('an undeclared number stays a number', () => {
  // With no `[nodes.*]` schema the original value is the only evidence of the
  // intended type; without it, `3` was written back as `"3"`.
  expect(display(3)).toBe('3');
  expect(parse('3', 3)).toBe(3);
  expect(parse('3.5', 1.5)).toBe(3.5);
  expect(unchanged(parse(display(3), 3), 3)).toBe(true);
});

test('a declared type wins over the original', () => {
  expect(parse('7', 'was a string', 'integer')).toBe(7);
});

test('text that is not the intended type is passed through, not coerced', () => {
  // `Number.parseInt('abc')` is NaN, which serializes to null — a removal. The
  // write boundary should refuse the value by name instead.
  expect(parse('abc', 3, 'integer')).toBe('abc');
  expect(parse('1.2.3', 1, 'number')).toBe('1.2.3');
  expect(parse('yes', true, 'boolean')).toBe('yes');
});

test('an emptied field is a removal', () => {
  expect(parse('', 'something')).toBe(null);
  expect(parse('   ', 3)).toBe(null);
});

test('a list with a comma inside an entry does not round-trip', () => {
  // `["Smith, Jane"]` joined and split becomes two people.
  expect(listRoundTrips(['alpha', 'beta'])).toBe(true);
  expect(listRoundTrips(['Smith, Jane'])).toBe(false);
  expect(listRoundTrips('not a list')).toBe(false);
});

test('unchanged recognises an untouched value of every shape', () => {
  expect(unchanged('x', 'x')).toBe(true);
  expect(unchanged(3, 3)).toBe(true);
  expect(unchanged(true, true)).toBe(true);
  expect(unchanged(['a', 'b'], ['a', 'b'])).toBe(true);
  expect(unchanged(null, undefined)).toBe(true);
  expect(unchanged('x', 'y')).toBe(false);
  expect(unchanged(3, '3')).toBe(false);
  expect(unchanged(['a'], ['a', 'b'])).toBe(false);
});
