import { expect, test } from 'bun:test';

import { metadataControl } from './test-dom';
import { documentMetadata, tomlSchemaResponse, type TomlSchemaResponse } from '../api';

const { renderMetadataForm, readMetadataForm } = await import('./metadata-form');

function draw(values: Record<string, unknown>, schema?: TomlSchemaResponse) {
  const metadata = documentMetadata(
    { aliases: [], labels: [], edges: {}, markdown: 'a.md', ...values },
    'fixture.metadata',
  );
  renderMetadataForm(
    { id: 'notes/a', type_folder: 'notes', markdown: 'body', html: '', metadata },
    schema,
  );
}

const schema = tomlSchemaResponse(
  {
    kind: 'note',
    schema: {},
    toml_template: '',
    constraints: {
      allowed_types: ['note'],
      allowed_status: ['active'],
      allowed_actors: [],
      allowed_edge_predicates: [],
      notes: [],
    },
    node_schema: {
      fields: {
        blank: { type: 'string', fields: {}, required: [], to: [] },
        template: { type: 'string', fields: {}, required: [], to: [] },
      },
      required: [],
    },
  },
  'fixture.schema',
);

test('body-only save omits untouched empty/whitespace strings and lossy lists', () => {
  for (const declared of [undefined, schema]) {
    draw(
      {
        type: 'note',
        status: '',
        occurred_at: ' 2026-01-01 ',
        blank: '',
        template: '  preserve whitespace  ',
        whitespace: '   ',
        aliases: ['', '  alias  ', ' '],
        labels: [' ', '', 'label '],
        enabled: true,
        priority: 3,
      },
      declared,
    );
    expect(metadataControl('blank').value).toBe('');
    expect(metadataControl('template').value).toBe('  preserve whitespace  ');
    expect(readMetadataForm()).toEqual({});
  }
});

test('unchanged reserved controls and comma-containing read-only lists stay untouched', () => {
  draw({ type: 'note', status: 'not-in-options', aliases: ['Smith, Jane'], labels: [''] });
  // Native selects cannot display an option they were never given. Comparing
  // the displayed value first must not turn that into an unsolicited clear.
  expect(metadataControl('Status').value).toBe('');
  expect(() => metadataControl('Aliases')).toThrow('No editable control');
  expect(readMetadataForm()).toEqual({});
});

test('deliberate edits still parse, and clearing populated fields still removes them', () => {
  draw(
    {
      type: 'note',
      status: 'active',
      occurred_at: '2026-01-01',
      blank: '',
      template: '  preserve whitespace  ',
      whitespace: '   ',
      enabled: true,
      priority: 3,
      aliases: ['', ' alias ', ' '],
      labels: ['label'],
    },
    schema,
  );
  metadataControl('Status').value = '';
  metadataControl('Occurred at').value = '';
  metadataControl('blank').value = 'new value';
  metadataControl('template').value = '  changed  ';
  metadataControl('whitespace').value = '';
  metadataControl('enabled').value = 'false';
  metadataControl('priority').value = '7';
  metadataControl('Aliases').value = ' first , second ';
  metadataControl('Labels').value = '';
  expect(readMetadataForm()).toEqual({
    status: null,
    occurred_at: null,
    aliases: ['first', 'second'],
    labels: [],
    fields: {
      blank: 'new value',
      template: 'changed',
      whitespace: null,
      enabled: false,
      priority: 7,
    },
  });
});

test('editing then restoring displayed text is not a metadata change', () => {
  draw({ type: 'note', template: '  original  ', aliases: ['', ' alias '] });
  metadataControl('template').value = 'changed';
  metadataControl('template').value = '  original  ';
  metadataControl('Aliases').value = 'changed';
  metadataControl('Aliases').value = ',  alias ';
  expect(readMetadataForm()).toEqual({});
});
