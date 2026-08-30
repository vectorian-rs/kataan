//! Folder icon lookup. Isolated so the lucide table is only pulled in by
//! modules that actually draw icons.

import {
  Archive,
  BookOpen,
  Boxes,
  Circle,
  Code,
  FileText,
  FolderKanban,
  Inbox,
  Lightbulb,
  ListTodo,
  Newspaper,
  Presentation,
  ReceiptText,
  User,
  type IconNode,
} from 'lucide';

function normalizeFolderIconKey(typeOrFolder: string) {
  const aliases: Record<string, string> = {
    intake: 'intake',
    notes: 'note',
    people: 'person',
    projects: 'project',
    topics: 'topic',
    articles: 'Newspaper',
    presentations: 'Presentation',
    references: 'BookOpen',
    finances: 'ReceiptText',
    tasks: 'ListTodo',
    type: 'type-definition',
  };
  return aliases[typeOrFolder] ?? typeOrFolder;
}

export function folderIcon(typeOrFolder: string): IconNode {
  const key = normalizeFolderIconKey(typeOrFolder);
  const icons: Record<string, IconNode> = {
    code: Code,
    note: FileText,
    person: User,
    project: FolderKanban,
    intake: Inbox,
    raw: Archive,
    topic: Lightbulb,
    'type-definition': Boxes,
    Inbox,
    Rocket: FolderKanban,
    Newspaper,
    Presentation,
    BookOpen,
    ReceiptText,
    ListTodo,
    FolderKanban,
    FileText,
    User,
    Code,
    Boxes,
    Lightbulb,
  };
  return icons[key] ?? Circle;
}
