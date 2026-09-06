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
import { readMetadataForm, renderMetadataForm } from './metadata-form';
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
}

export function beginEditing() {
  if (!openDocument) return;
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
export async function saveEditing(reopen: (id: string) => Promise<void>) {
  if (!openDocument) return;
  // Body and metadata in one call: `update_document` applies them together, so
  // a save either lands whole or is refused whole.
  //
  // The precondition is always sent, empty when the document has no
  // `updated_at` — which is most of them in a hand-authored vault. Omitting it
  // would mean no check at all, and the save could then overwrite an edit made
  // while this tab sat open.
  await updateDocument(
    openDocument.id,
    { body: documentEditor.value, ...readMetadataForm() },
    openDocument.updatedAt ?? '',
  );

  // Re-read rather than patching the DOM: the server re-renders the Markdown,
  // and `updated_at` has moved — keeping the stale one would make the *next*
  // save fail its own precondition.
  editing = false;
  await reopen(openDocument.id);
}
