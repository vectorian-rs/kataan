import { documentBody } from './elements';
//! What a non-document file looks like in the reader: a `kind` dispatch onto
//! the document body, plus the theme the highlighter should use.

import { getRawFileUrl, type FileResponse } from '../api';

export function currentTheme() {
  return document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light';
}

export function renderHighlightedFile(html: string) {
  documentBody.className = 'reader-body file-reader-body';
  documentBody.innerHTML = html;
}

export function renderFileBody(file: FileResponse) {
  documentBody.className = 'reader-body';
  documentBody.innerHTML = '';

  if (file.kind === 'json') {
    const pre = document.createElement('pre');
    pre.className = 'file-preview json-preview';
    try {
      pre.textContent = JSON.stringify(JSON.parse(file.content), null, 2);
    } catch {
      pre.textContent = file.content;
    }
    documentBody.append(pre);
    return;
  }

  if (file.kind === 'html') {
    documentBody.className = 'reader-body file-reader-body html-reader-body';
    const frame = document.createElement('iframe');
    frame.className = 'html-preview';
    frame.title = file.name;
    frame.setAttribute('sandbox', 'allow-scripts');
    frame.srcdoc = file.content;
    documentBody.append(frame);
    return;
  }

  if (file.kind === 'text') {
    const pre = document.createElement('pre');
    pre.className = 'file-preview';
    pre.textContent = file.content;
    documentBody.append(pre);
    return;
  }

  if (file.kind === 'pdf') {
    documentBody.className = 'reader-body file-reader-body pdf-reader-body';
    const frame = document.createElement('iframe');
    frame.className = 'pdf-preview';
    frame.title = file.name;
    frame.src = `${getRawFileUrl(file.path)}#toolbar=0&navpanes=0&scrollbar=0`;
    documentBody.append(frame);
    return;
  }

  if (file.kind === 'image') {
    documentBody.className = 'reader-body file-reader-body image-reader-body';
    const preview = document.createElement('img');
    preview.className = 'image-preview';
    preview.src = getRawFileUrl(file.path);
    preview.alt = file.name;
    documentBody.append(preview);
    return;
  }

  documentBody.className = 'reader-body empty-state';
  const card = document.createElement('div');
  card.className = 'empty-card';
  const title = document.createElement('strong');
  title.textContent = 'Preview not available';
  const detail = document.createElement('span');
  detail.textContent = file.path;
  card.append(title, detail);
  documentBody.append(card);
}
