//! Minimal control tree for exercising production form/editor functions in Bun.
//! This is not a browser: layout, focus and native event dispatch need browser checks.

export class TestElement {
  children: TestElement[] = [];
  textContent = '';
  innerHTML = '';
  className = '';
  hidden = false;
  disabled = false;
  type = '';
  placeholder = '';
  scrollTop = 0;
  dataset: Record<string, string> = {};
  classList = {
    add: (...names: string[]) => {
      this.className += ` ${names.join(' ')}`;
    },
  };
  private rawValue = '';

  constructor(readonly tagName = 'div') {}

  get value() {
    return this.rawValue;
  }

  set value(value: string) {
    this.rawValue =
      this.tagName === 'select' && !this.children.some((option) => option.value === value)
        ? ''
        : value;
  }

  append(...children: TestElement[]) {
    this.children.push(...children);
  }

  replaceChildren(...children: TestElement[]) {
    this.children = children;
  }

  querySelector(selector: string): TestElement | null {
    return this.querySelectorAll(selector)[0] ?? null;
  }

  querySelectorAll(selector: string): TestElement[] {
    const selectors = selector.split(',').map((part) => part.trim());
    return this.children.flatMap((child) => [
      ...(selectors.some((part) =>
        part.startsWith('.')
          ? child.className.split(' ').includes(part.slice(1))
          : child.tagName === part,
      )
        ? [child]
        : []),
      ...child.querySelectorAll(selector),
    ]);
  }

  focus() {}
  setSelectionRange() {}
}

const elements = new Map<string, TestElement>();

export function element(id: string): TestElement {
  let found = elements.get(id);
  if (!found) {
    found = new TestElement(id === 'document-editor' ? 'textarea' : 'div');
    elements.set(id, found);
  }
  return found;
}

Object.defineProperty(globalThis, 'document', {
  configurable: true,
  value: {
    createElement: (tag: string) => new TestElement(tag),
    getElementById: element,
    documentElement: new TestElement('html'),
  },
});

export function metadataControl(label: string): TestElement {
  const row = element('metadata-panel')
    .querySelectorAll('.metadata-field')
    .find((row) => row.children[0]?.textContent === label);
  const control = row?.children[1];
  if (!control || !['input', 'select'].includes(control.tagName)) {
    throw new Error(`No editable control for ${label}`);
  }
  return control;
}
