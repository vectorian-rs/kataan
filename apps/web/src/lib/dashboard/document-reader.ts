//! The document fetch/render transition, shared by navigation and save refresh.

import { getDocument, getSchema, type DocumentResponse, type TomlSchemaResponse } from '../api';
import { breadcrumb, documentBody, documentTitle } from './elements';
import { setOpenDocument } from './editing';
import { currentTheme } from './file-preview';
import { basenameFromId } from './format';
import { beginNavigation, type Stale } from './navigation';
import { renderMetadata, renderSchema } from './panels';
import { setRoute } from './routes';

/// One fetch per type, until a vault/ontology reload clears the caches.
const typeSchemas = new Map<string, Promise<TomlSchemaResponse>>();
let documentSchemaRequest: ReturnType<typeof getSchema> | undefined;

function typeSchemaFor(vaultDocument: DocumentResponse) {
  const type = String(vaultDocument.metadata.type ?? '');
  let request = typeSchemas.get(type);
  if (!request) {
    request = getSchema(type);
    typeSchemas.set(type, request);
  }
  return request;
}

function documentSchema() {
  documentSchemaRequest ??= getSchema('document');
  return documentSchemaRequest;
}

export function forgetDocumentSchema() {
  documentSchemaRequest = undefined;
  typeSchemas.clear();
}

export async function showDocument(
  id: string,
  options: { updateUrl?: boolean; stale?: Stale } = {},
) {
  const updateUrl = options.updateUrl ?? true;
  const stale = options.stale ?? beginNavigation();
  while (!stale()) {
    const theme = currentTheme();
    const [vaultDocument, schema] = await Promise.all([getDocument(id, theme), documentSchema()]);
    if (stale()) return;
    // The type's schema carries node_schema; the generic one describes kataan's
    // own keys. A type without declared fields still has an editable form.
    const typeSchema = await typeSchemaFor(vaultDocument).catch(() => undefined);
    if (stale()) return;
    // Editing stays locked during Save refresh, so its theme-change listener
    // cannot reload the reader. Check after *all* awaits, including schemas.
    // Retry without advancing navigation or publishing an obsolete rendering.
    if (theme !== currentTheme()) continue;
    breadcrumb.textContent = vaultDocument.id.replaceAll('/', ' › ');
    documentTitle.textContent = basenameFromId(vaultDocument.id);
    if (updateUrl) {
      setRoute({ kind: 'id', id: vaultDocument.id });
    }
    setOpenDocument({
      id: vaultDocument.id,
      markdown: vaultDocument.markdown,
      updatedAt:
        typeof vaultDocument.metadata.updated_at === 'string'
          ? vaultDocument.metadata.updated_at
          : undefined,
      document: vaultDocument,
      schema: typeSchema,
    });
    documentBody.className = 'reader-body';
    documentBody.innerHTML = vaultDocument.html;
    renderMetadata(vaultDocument);
    renderSchema(schema);
    return;
  }
}
