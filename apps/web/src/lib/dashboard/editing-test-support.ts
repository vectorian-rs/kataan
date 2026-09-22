//! Transport/history fixtures only: reader, editor, form, API parsing and
//! navigation ownership in these tests are the production functions.

import { spyOn } from 'bun:test';
import { documentResponse } from '../api';

export function fixtureDocument(id: string, body = `body ${id}`, stamp = 'old') {
  return documentResponse(
    {
      id,
      type_folder: 'notes',
      markdown: body,
      html: `<p>${body}</p>`,
      metadata: {
        type: 'note',
        markdown: `${id.split('/').at(-1)}.md`,
        aliases: [],
        labels: [],
        edges: {},
        updated_at: stamp,
        status: 'active',
        occurred_at: '2026-01-01',
        custom: 'original',
      },
    },
    'fixture',
  );
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

type PendingResponse = {
  started: ReturnType<typeof deferred<void>>;
  response: ReturnType<typeof deferred<Response>>;
};

export const fixtureSchema = {
  kind: 'document',
  schema: {},
  toml_template: 'type = "note"',
  constraints: {
    allowed_types: ['note'],
    allowed_status: ['active'],
    allowed_actors: [],
    allowed_edge_predicates: [],
    notes: [],
  },
};

export class ReaderTransport {
  requests: {
    method: string;
    path: string;
    theme: string | null;
    body: Record<string, unknown>;
  }[] = [];
  documents = new Map([
    ['notes/a', fixtureDocument('notes/a')],
    ['notes/b', fixtureDocument('notes/b')],
  ]);
  private queued = new Map<string, PendingResponse[]>();
  private fetchSpy = spyOn(globalThis, 'fetch').mockImplementation(
    Object.assign(
      async (input: Parameters<typeof fetch>[0], init?: Parameters<typeof fetch>[1]) => {
        const url = input instanceof Request ? input.url : String(input);
        const parsed = new URL(url, 'http://fixture.invalid');
        const path = parsed.pathname;
        const method = init?.method ?? 'GET';
        this.requests.push({
          method,
          path,
          theme: parsed.searchParams.get('theme'),
          body: init?.body ? JSON.parse(String(init.body)) : {},
        });
        const pending = this.queued.get(`${method} ${path}`)?.shift();
        if (pending) {
          pending.started.resolve(undefined);
          return pending.response.promise;
        }
        if (path.startsWith('/api/schema/')) return Response.json(fixtureSchema);
        const doc = this.documents.get(path.replace('/api/documents/', ''));
        if (method === 'GET' && doc) return Response.json(doc);
        throw new Error(`Unexpected request: ${method} ${path}`);
      },
      { preconnect: fetch.preconnect },
    ),
  );

  defer(method: string, id: string): PendingResponse {
    return this.deferPath(method, `/api/documents/${id}`);
  }

  deferPath(method: string, path: string): PendingResponse {
    const pending = { started: deferred<void>(), response: deferred<Response>() };
    const key = `${method} ${path}`;
    const queue = this.queued.get(key) ?? [];
    queue.push(pending);
    this.queued.set(key, queue);
    return pending;
  }

  count(method: string, id: string) {
    return this.requests.filter((r) => r.method === method && r.path === `/api/documents/${id}`)
      .length;
  }

  restore() {
    this.fetchSpy.mockRestore();
  }
}

export function installHistory() {
  const entries = ['/'];
  const location = { pathname: '/', search: '' };
  const set = (path: string) => {
    const url = new URL(path, 'http://fixture.invalid');
    location.pathname = url.pathname;
    location.search = url.search;
  };
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value: {
      location,
      history: {
        pushState: (_data: unknown, _unused: string, path: string) => {
          entries.push(path);
          set(path);
        },
        replaceState: (_data: unknown, _unused: string, path: string) => {
          entries[entries.length - 1] = path;
          set(path);
        },
      },
    },
  });
  return { entries, location };
}
