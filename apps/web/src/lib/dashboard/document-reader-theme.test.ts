import { afterEach, beforeEach, expect, test } from 'bun:test';

import { element, metadataControl } from './test-dom';
import {
  fixtureDocument,
  fixtureSchema,
  installHistory,
  ReaderTransport,
} from './editing-test-support';
import { beginNavigation, currentNavigation, type Stale } from './navigation';

const { showDocument, forgetDocumentSchema } = await import('./document-reader');
const { beginEditing, cancelEditing, isEditing, saveEditing, setOpenDocument } =
  await import('./editing');
const { readMetadataForm } = await import('./metadata-form');

let transport: ReaderTransport;
let history: ReturnType<typeof installHistory>;
const editor = element('document-editor');
const refresh = (id: string, stale: Stale) => showDocument(id, { stale, updateUrl: false });
const theme = (value: string) => {
  document.documentElement.dataset.theme = value;
};
const rendered = (value: string) =>
  Response.json({ ...fixtureDocument('notes/a', 'A saved', 'new'), html: `<pre>${value}</pre>` });
const ok = () => Response.json({ ok: true });

beforeEach(async () => {
  transport = new ReaderTransport();
  history = installHistory();
  forgetDocumentSchema();
  theme('light');
  await showDocument('notes/a');
  beginEditing();
  editor.value = 'A draft';
  metadataControl('custom').value = 'A metadata';
});

afterEach(() => {
  beginNavigation();
  setOpenDocument(null);
  theme('light');
  transport.restore();
});

function expectLockedDraft() {
  expect(isEditing()).toBe(true);
  expect(editor.value).toBe('A draft');
  expect(readMetadataForm()).toEqual({ fields: { custom: 'A metadata' } });
  for (const id of ['document-editor', 'edit-button', 'save-button', 'cancel-button']) {
    expect(element(id).disabled).toBe(true);
  }
  for (const control of element('metadata-panel').querySelectorAll('input, select')) {
    expect(control.disabled).toBe(true);
  }
  expect(element('document-body').innerHTML).toBe('<p>body notes/a</p>');
}

async function savingRefresh() {
  const patch = transport.defer('PATCH', 'notes/a');
  const get = transport.defer('GET', 'notes/a');
  const pending = saveEditing(refresh);
  patch.response.resolve(ok());
  await get.started.promise;
  return { get, pending };
}

function requestedThemes() {
  return transport.requests
    .filter((r) => r.method === 'GET' && r.path === '/api/documents/notes/a')
    .map((r) => r.theme);
}

test('Save retries each changed theme without publishing obsolete HTML or releasing its lock', async () => {
  const ownership = currentNavigation();
  const { get, pending } = await savingRefresh();
  theme('dark');
  const dark = transport.defer('GET', 'notes/a');
  get.response.resolve(rendered('light'));
  await Promise.race([dark.started.promise, pending]);
  expect(requestedThemes()).toEqual(['light', 'light', 'dark']);
  expectLockedDraft();
  cancelEditing();
  beginEditing();
  await saveEditing(refresh);
  expect(transport.count('PATCH', 'notes/a')).toBe(1);
  expectLockedDraft();

  theme('light');
  const light = transport.defer('GET', 'notes/a');
  dark.response.resolve(rendered('dark'));
  await Promise.race([light.started.promise, pending]);
  expect(requestedThemes()).toEqual(['light', 'light', 'dark', 'light']);
  expectLockedDraft();
  light.response.resolve(rendered('light-current'));
  await pending;
  expect(ownership()).toBe(false);
  expect(isEditing()).toBe(false);
  expect(editor.disabled).toBe(false);
  expect(element('document-body').innerHTML).toBe('<pre>light-current</pre>');
  expect(history.entries).toEqual(['/', '/notes/a']);
  beginEditing();
  const next = transport.defer('PATCH', 'notes/a');
  const nextSave = saveEditing(refresh);
  expect(transport.requests.filter((r) => r.method === 'PATCH')[1].body.expected_updated_at).toBe(
    'new',
  );
  next.response.resolve(ok());
  await nextSave;
});

for (const schema of ['document', 'note']) {
  test(`theme is checked after deferred ${schema} schema as well as document GET`, async () => {
    forgetDocumentSchema();
    const delayed = transport.deferPath('GET', `/api/schema/${schema}`);
    const { get, pending } = await savingRefresh();
    get.response.resolve(rendered('light'));
    await delayed.started.promise;
    // GET has already completed; only schema work is keeping the reader open.
    theme('dark');
    const correction = transport.defer('GET', 'notes/a');
    delayed.response.resolve(Response.json(fixtureSchema));
    await Promise.race([correction.started.promise, pending]);
    expect(requestedThemes()).toEqual(['light', 'light', 'dark']);
    expectLockedDraft();
    correction.response.resolve(rendered('dark'));
    await pending;
    expect(element('document-body').innerHTML).toBe('<pre>dark</pre>');
  });
}

for (const outcome of ['success', 'http-error', 'network-error']) {
  test(`navigation away during theme correction owns the reader after ${outcome}`, async () => {
    const { get, pending } = await savingRefresh();
    theme('dark');
    const correction = transport.defer('GET', 'notes/a');
    get.response.resolve(rendered('light'));
    await Promise.race([correction.started.promise, pending]);
    expect(requestedThemes()).toEqual(['light', 'light', 'dark']);
    await showDocument('notes/b');
    beginEditing();
    editor.value = 'B draft';
    metadataControl('custom').value = 'B metadata';
    const patchB = transport.defer('PATCH', 'notes/b');
    const savingB = saveEditing(refresh);
    theme('light'); // stale A must not issue another corrective request
    if (outcome === 'success') correction.response.resolve(rendered('dark'));
    else if (outcome === 'http-error')
      correction.response.resolve(new Response('failed', { status: 500 }));
    else correction.response.reject(new Error('offline'));
    await pending;
    expect(requestedThemes()).toEqual(['light', 'light', 'dark']);
    expect(history.location.pathname).toBe('/notes/b');
    expect(element('breadcrumb').textContent).toBe('notes › b');
    expect(editor.value).toBe('B draft');
    expect(readMetadataForm()).toEqual({ fields: { custom: 'B metadata' } });
    expect(isEditing()).toBe(true);
    expect(editor.disabled).toBe(true); // A may not unlock B's Save
    patchB.response.resolve(ok());
    await savingB;
    expect(editor.disabled).toBe(false);
  });
}

for (const outcome of ['http-error', 'network-error']) {
  test(`theme correction ${outcome} preserves same-document draft and old revision`, async () => {
    const { get, pending } = await savingRefresh();
    const failure = pending.then(
      () => undefined,
      (error: unknown) => error,
    );
    theme('dark');
    const correction = transport.defer('GET', 'notes/a');
    get.response.resolve(rendered('light'));
    await Promise.race([correction.started.promise, pending]);
    expect(requestedThemes()).toEqual(['light', 'light', 'dark']);
    expectLockedDraft();
    if (outcome === 'http-error')
      correction.response.resolve(new Response('failed', { status: 500 }));
    else correction.response.reject(new Error('offline'));
    expect(await failure).toBeInstanceOf(Error);
    expect(editor.disabled).toBe(false);
    expect(isEditing()).toBe(true);
    expect(editor.value).toBe('A draft');
    expect(readMetadataForm()).toEqual({ fields: { custom: 'A metadata' } });
    expect(element('document-body').innerHTML).toBe('<p>body notes/a</p>');
    const retry = transport.defer('PATCH', 'notes/a');
    const retried = saveEditing(refresh);
    expect(transport.requests.filter((r) => r.method === 'PATCH')[1].body.expected_updated_at).toBe(
      'old',
    );
    retry.response.resolve(ok());
    await retried;
  });
}
