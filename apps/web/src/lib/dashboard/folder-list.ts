//! Restoring search's list pane is not a reader navigation. Borrow ownership
//! so a later navigation can still discard the folder response.

import { getFolder, type CanonicalFolderResponse } from '../api';
import { currentNavigation } from './navigation';

export async function restoreFolderList(
  folder: string,
  apply: (response: CanonicalFolderResponse) => void,
) {
  const stale = currentNavigation();
  const response = await getFolder(folder);
  if (stale()) return;
  apply(response);
}
