//! Edit mode: which controls are showing, and what a save sends.
//!
//! One mode covering both halves of a document. Entering it swaps the reader
//! for the Markdown textarea *and* the properties panel for a form; leaving it
//! puts both back. Body and metadata then go in a single `update_document`, so
//! a save lands whole or is refused whole — two calls would allow the body to
//! be written and the metadata to then fail validation, leaving the document in
//! a state the vault rejects.

import { type DocumentResponse, type TomlSchemaResponse, updateDocument } from '../api';

import { cancelButton, documentBody, documentEditor, editButton, saveButton } from './elements';
import { readMetadataForm, renderMetadataForm, setMetadataFormDisabled } from './metadata-form';
import { currentNavigation, type Stale } from './navigation';
import { renderMetadata } from './panels';

/// The document currently open in the reader, when it is one.
///
/// Editing needs two things a rendered document does not carry: the Markdown
/// source, and the `updated_at` it was read at — the precondition the server
/// checks so a save cannot overwrite a change this tab never saw.
export interface OpenDocument {
  id: string;
  markdown: string;
  updatedAt?: string;
  /// Kept so entering edit mode renders the form without a second fetch, and
  /// Cancel restores the panel from what was already displayed.
  document: DocumentResponse;
  /// The type's schema, when it declares fields. Absent is fine — the form then
  /// offers whatever keys the document already carries.
  schema?: TomlSchemaResponse;
}

let openDocument: OpenDocument | null = null;
let editing = false;
// Keyed by id rather than object identity: leaving and returning to A must not
// allow a second save while A's first is pending. B can still be edited/saved.
const pendingSaves = new Set<string>();

function isSaving() {
  return openDocument !== null && pendingSaves.has(openDocument.id);
}

/// What the reader is showing, or `null` for a file or an empty pane. Always
/// leaves edit mode: whatever was being edited is no longer what is on screen.
export function setOpenDocument(document: OpenDocument | null) {
  openDocument = document;
  editing = false;
  renderEditControls();
}

/// Show the controls that apply right now: nothing without a document, Edit
/// when reading one, Save/Cancel while editing.
export function renderEditControls() {
  const hasDocument = openDocument !== null;
  editButton.hidden = !hasDocument || editing;
  saveButton.hidden = !editing;
  cancelButton.hidden = !editing;
  documentEditor.hidden = !editing;
  documentBody.hidden = editing;
  const disabled = isSaving();
  documentEditor.disabled = disabled;
  editButton.disabled = disabled;
  saveButton.disabled = disabled;
  cancelButton.disabled = disabled;
  setMetadataFormDisabled(disabled);
}

export function beginEditing() {
  if (!openDocument || isSaving()) return;
  editing = true;
  renderMetadataForm(openDocument.document, openDocument.schema);
  documentEditor.value = openDocument.markdown;
  renderEditControls();
  // Caret at the start, not wherever focus lands — otherwise the view opens
  // scrolled past the first lines of the document you just chose to edit.
  documentEditor.focus();
  documentEditor.setSelectionRange(0, 0);
  documentEditor.scrollTop = 0;
}

/// Leave the editor without saving. The rendered body is still in the DOM
/// underneath, so there is nothing to re-fetch.
export function cancelEditing() {
  if (isSaving()) return;
  if (openDocument) {
    renderMetadata(openDocument.document);
  }
  editing = false;
  renderEditControls();
}

/// Save body and metadata together, then hand the document back to `reopen`.
///
/// `reopen` rather than a direct call to `selectDocument`: the selection layer
/// already imports this module to drive the controls, and importing it back
/// would make the two mutually dependent — the exact shape that leaves one side
/// holding an `undefined` binding at boot.
export async function saveEditing(reopen: (id: string, stale: Stale) => Promise<void>) {
  // Captured before the request, not read again after it. A save is a request
  // about *this* document, and `openDocument` is whatever is on screen now —
  // if the reader moved on while the PATCH was in flight, the completion used
  // to reopen the new document and reset the draft someone had started in it.
  const saving = openDocument;
  if (!saving || !editing || isSaving()) return;
  const navigationStale = currentNavigation();
  const stale = () => navigationStale() || openDocument !== saving;
  pendingSaves.add(saving.id);
  renderEditControls();

  // Body and metadata in one call: `update_document` applies them together, so
  // a save either lands whole or is refused whole.
  //
  // The precondition is always sent, empty when the document has no
  // `updated_at` — which is most of them in a hand-authored vault. Omitting it
  // would mean no check at all, and the save could then overwrite an edit made
  // while this tab sat open.
  try {
    await updateDocument(
      saving.id,
      { body: documentEditor.value, ...readMetadataForm() },
      saving.updatedAt ?? '',
    );

    // Identity alone misses a newer navigation whose GET has not finished.
    // Borrow the navigation token for refresh, never supersede a user's click.
    if (stale()) return;
    // Stay in edit mode, locked, until the reader successfully replaces it.
    // Either PATCH or refresh failure must leave the draft available.
    await reopen(saving.id, stale);
  } catch (error) {
    // A late failure belongs to A, not to the reader/draft now open in B.
    if (!stale()) throw error;
  } finally {
    pendingSaves.delete(saving.id);
    renderEditControls();
  }
}

/// Whether a draft is open that a refresh would discard.
///
/// Re-selecting a document rebuilds the reader and the properties panel from
/// disk, which is right for navigation and wrong for anything incidental — a
/// theme change would otherwise throw away unsaved text.
export function isEditing() {
  return editing;
}
