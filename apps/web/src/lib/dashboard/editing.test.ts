import { afterEach, beforeEach, expect, test } from 'bun:test';

import { element, metadataControl } from './test-dom';
import { fixtureDocument, installHistory, ReaderTransport } from './editing-test-support';
import { beginNavigation, type Stale } from './navigation';

const { showDocument, forgetDocumentSchema } = await import('./document-reader');
const { beginEditing, cancelEditing, isEditing, saveEditing, setOpenDocument } =
  await import('./editing');
const { readMetadataForm } = await import('./metadata-form');

let transport: ReaderTransport;
let history: ReturnType<typeof installHistory>;
const editor = element('document-editor');
const refresh = (id: string, stale: Stale) => showDocument(id, { stale, updateUrl: false });
const ok = () => Response.json({ ok: true });

beforeEach(async () => {
  transport = new ReaderTransport();
  history = installHistory();
  forgetDocumentSchema();
  await showDocument('notes/a');
  beginEditing();
  editor.value = 'A draft';
});

afterEach(() => {
  beginNavigation();
  setOpenDocument(null);
  transport.restore();
});

async function expectFailure(pending: Promise<void>, message?: string) {
  const error: unknown = await pending.then(
    () => undefined,
    (error: unknown) => error,
  );
  expect(error).toBeInstanceOf(Error);
  if (message && error instanceof Error) expect(error.message).toContain(message);
}

function expectLocked(locked: boolean) {
  for (const id of ['document-editor', 'edit-button', 'save-button', 'cancel-button']) {
    expect(element(id).disabled).toBe(locked);
  }
  for (const control of element('metadata-panel').querySelectorAll('input, select')) {
    expect(control.disabled).toBe(locked);
  }
}

function expectBDraft() {
  expect(history.location.pathname).toBe('/notes/b');
  expect(element('breadcrumb').textContent).toBe('notes › b');
  expect(isEditing()).toBe(true);
  expect(editor.value).toBe('B draft');
  expect(readMetadataForm()).toEqual({ fields: { custom: 'B metadata' } });
}

async function editB() {
  await showDocument('notes/b');
  beginEditing();
  editor.value = 'B draft';
  metadataControl('custom').value = 'B metadata';
}

test('same-document controls and duplicate saves are blocked through PATCH and refresh GET', async () => {
  metadataControl('Status').value = '';
  metadataControl('Occurred at').value = '';
  metadataControl('custom').value = '';
  const patch = transport.defer('PATCH', 'notes/a');
  const pending = saveEditing(refresh);
  expectLocked(true);
  beginEditing();
  cancelEditing();
  await saveEditing(refresh);
  expect(isEditing()).toBe(true);
  expect(editor.value).toBe('A draft');
  expect(transport.count('PATCH', 'notes/a')).toBe(1);
  expect(transport.requests.find((r) => r.method === 'PATCH')?.body).toEqual({
    body: 'A draft',
    status: null,
    occurred_at: null,
    fields: { custom: null },
    expected_updated_at: 'old',
  });

  const get = transport.defer('GET', 'notes/a');
  patch.response.resolve(ok());
  await get.started.promise;
  expectLocked(true);
  expect(isEditing()).toBe(true);
  await saveEditing(refresh);
  cancelEditing();
  expect(transport.count('PATCH', 'notes/a')).toBe(1);
  get.response.resolve(Response.json(fixtureDocument('notes/a', 'A draft', 'new')));
  await pending;
  expectLocked(false);
  expect(isEditing()).toBe(false);
  expect(element('document-body').innerHTML).toBe('<p>A draft</p>');
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

test('A completion cannot supersede navigation to B whose GET is still pending', async () => {
  const patch = transport.defer('PATCH', 'notes/a');
  const pending = saveEditing(refresh);
  const getB = transport.defer('GET', 'notes/b');
  const selectingB = showDocument('notes/b');
  await getB.started.promise;
  patch.response.resolve(ok());
  await pending;
  expect(transport.count('GET', 'notes/a')).toBe(1); // initial load only
  getB.response.resolve(Response.json(fixtureDocument('notes/b')));
  await selectingB;
  expect(history.location.pathname).toBe('/notes/b');
  expect(element('breadcrumb').textContent).toBe('notes › b');
  expect(element('document-body').innerHTML).toBe('<p>body notes/b</p>');
});

test('a deferred A refresh cannot discard a loaded B draft', async () => {
  const patch = transport.defer('PATCH', 'notes/a');
  const pending = saveEditing(refresh);
  const getA = transport.defer('GET', 'notes/a');
  patch.response.resolve(ok());
  await getA.started.promise;
  await editB();
  expectLocked(false);
  getA.response.resolve(Response.json(fixtureDocument('notes/a', 'A draft', 'new')));
  await pending;
  expectBDraft();
  expectLocked(false);
  expect(history.entries).toEqual(['/', '/notes/a', '/notes/b']);
});

test('a save begun while navigation is loading cannot later replace its loaded document', async () => {
  const getB = transport.defer('GET', 'notes/b');
  const selectingB = showDocument('notes/b');
  const patchA = transport.defer('PATCH', 'notes/a');
  const pending = saveEditing(refresh);
  const getA = transport.defer('GET', 'notes/a');
  patchA.response.resolve(ok());
  await getA.started.promise;
  getB.response.resolve(Response.json(fixtureDocument('notes/b')));
  await selectingB;
  beginEditing();
  editor.value = 'B draft';
  metadataControl('custom').value = 'B metadata';
  getA.response.resolve(Response.json(fixtureDocument('notes/a', 'A draft', 'new')));
  await pending;
  expectBDraft();
  expectLocked(false);
});

test('B stays editable during A PATCH and its draft survives A completion', async () => {
  const patch = transport.defer('PATCH', 'notes/a');
  const pending = saveEditing(refresh);
  await editB();
  expectLocked(false);
  patch.response.resolve(ok());
  await pending;
  expectBDraft();
  expectLocked(false);
});

test('overlapping saves belong to their own document; A cannot unlock pending B', async () => {
  const patchA = transport.defer('PATCH', 'notes/a');
  const pendingA = saveEditing(refresh);
  await editB();
  const patchB = transport.defer('PATCH', 'notes/b');
  const pendingB = saveEditing(refresh);
  expectLocked(true);
  patchA.response.resolve(ok());
  await pendingA;
  expectBDraft();
  expectLocked(true);
  await saveEditing(refresh);
  expect(transport.count('PATCH', 'notes/b')).toBe(1);
  const getB = transport.defer('GET', 'notes/b');
  patchB.response.resolve(ok());
  await getB.started.promise;
  expectLocked(true);
  getB.response.resolve(Response.json(fixtureDocument('notes/b', 'B draft', 'new-b')));
  await pendingB;
  expectLocked(false);
  expect(isEditing()).toBe(false);
  expect(history.location.pathname).toBe('/notes/b');
  expect(element('document-body').innerHTML).toBe('<p>B draft</p>');
});

test('away and back to A still blocks duplicate A saves and invalidates its old refresh', async () => {
  const patch = transport.defer('PATCH', 'notes/a');
  const pending = saveEditing(refresh);
  await editB();
  await showDocument('notes/a');
  expectLocked(true);
  beginEditing();
  await saveEditing(refresh);
  expect(isEditing()).toBe(false);
  expect(transport.count('PATCH', 'notes/a')).toBe(1);
  patch.response.resolve(ok());
  await pending;
  expect(transport.count('GET', 'notes/a')).toBe(2); // load and user return, no old refresh
  expectLocked(false);
  beginEditing();
  expect(isEditing()).toBe(true);
});

for (const failure of [409, 500, 'network'] as const) {
  test(`PATCH ${failure} preserves the draft and restores controls for retry`, async () => {
    metadataControl('custom').value = 'draft metadata';
    const patch = transport.defer('PATCH', 'notes/a');
    const pending = saveEditing(refresh);
    if (failure === 'network') patch.response.reject(new Error('offline'));
    else patch.response.resolve(new Response('refused', { status: failure }));
    await expectFailure(pending);
    expectLocked(false);
    expect(isEditing()).toBe(true);
    expect(editor.value).toBe('A draft');
    expect(readMetadataForm()).toEqual({ fields: { custom: 'draft metadata' } });
    const retry = transport.defer('PATCH', 'notes/a');
    const retried = saveEditing(refresh);
    expectLocked(true);
    retry.response.resolve(ok());
    await retried;
    expect(transport.count('PATCH', 'notes/a')).toBe(2);
    expectLocked(false);
    expect(isEditing()).toBe(false);
  });
}

test('failed refresh preserves draft, restores controls and never invents a new precondition', async () => {
  const patch = transport.defer('PATCH', 'notes/a');
  const pending = saveEditing(refresh);
  const getA = transport.defer('GET', 'notes/a');
  patch.response.resolve(ok());
  await getA.started.promise;
  getA.response.resolve(new Response('unavailable', { status: 500 }));
  await expectFailure(pending);
  expectLocked(false);
  expect(isEditing()).toBe(true);
  expect(editor.value).toBe('A draft');
  const retry = transport.defer('PATCH', 'notes/a');
  const retried = saveEditing(refresh);
  retry.response.resolve(new Response('', { status: 409 }));
  await expectFailure(retried, 'changed on disk');
  expect(transport.requests.filter((r) => r.method === 'PATCH')[1].body.expected_updated_at).toBe(
    'old',
  );
  expectLocked(false);
  expect(isEditing()).toBe(true);
  expect(editor.value).toBe('A draft');
});

test('late A errors cannot replace or unlock the reader/draft now owned by B', async () => {
  const patchA = transport.defer('PATCH', 'notes/a');
  const pendingA = saveEditing(refresh);
  await editB();
  const patchB = transport.defer('PATCH', 'notes/b');
  const pendingB = saveEditing(refresh);
  patchA.response.resolve(new Response('unavailable', { status: 500 }));
  await pendingA; // must not propagate an error to dashboard's B-owned error renderer
  expectBDraft();
  expectLocked(true);
  patchB.response.resolve(ok());
  await pendingB;
});

test('clearing the reader while saving cannot reopen A or leave editing controls visible', async () => {
  const patch = transport.defer('PATCH', 'notes/a');
  const pending = saveEditing(refresh);
  beginNavigation();
  setOpenDocument(null);
  patch.response.resolve(ok());
  await pending;
  expect(isEditing()).toBe(false);
  expect(transport.count('GET', 'notes/a')).toBe(1);
  for (const id of ['edit-button', 'save-button', 'cancel-button', 'document-editor']) {
    expect(element(id).hidden).toBe(true);
  }
  await saveEditing(refresh);
  expect(transport.count('PATCH', 'notes/a')).toBe(1);
});

// Exercise search clearing through its production list-restoration transition,
// sharing the real navigation token with the production Save/reader pipeline.
for (const navigate of [false, true]) {
  test(`search clear during Save ${navigate ? 'yields to later navigation' : 'refreshes the same editor revision'}`, async () => {
    const { runSearch } = await import('./search');
    const { restoreFolderList } = await import('./folder-list');
    const patch = transport.defer('PATCH', 'notes/a');
    const pending = saveEditing(refresh);
    const folder = transport.deferPath('GET', '/api/folders/notes');
    const rendered: string[] = [];
    const clearing = runSearch('', {
      restoreFolder: () => restoreFolderList('notes', (response) => rendered.push(response.id)),
      openDocument: async () => {},
      openFolder: async () => {},
      openFile: async () => {},
      run: () => {},
    });
    await folder.started.promise;
    expectLocked(true);
    if (navigate) await editB();
    folder.response.resolve(Response.json({ id: 'notes', documents: [], files: [], folders: [] }));
    await clearing;
    expect(rendered).toEqual(navigate ? [] : ['notes']);
    transport.documents.set('notes/a', fixtureDocument('notes/a', 'A draft', 'saved'));
    patch.response.resolve(ok());
    await pending;
    if (navigate) {
      expectBDraft();
      expect(transport.count('GET', 'notes/a')).toBe(1);
    } else {
      expect(transport.count('GET', 'notes/a')).toBe(2);
      expect(isEditing()).toBe(false);
      expect(history.entries).toEqual(['/', '/notes/a']);
      beginEditing();
      const next = transport.defer('PATCH', 'notes/a');
      const nextSave = saveEditing(refresh);
      expect(
        transport.requests.filter((r) => r.method === 'PATCH')[1].body.expected_updated_at,
      ).toBe('saved');
      next.response.resolve(ok());
      await nextSave;
    }
    expectLocked(false);
  });
}
