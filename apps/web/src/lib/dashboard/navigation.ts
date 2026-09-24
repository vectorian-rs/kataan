//! Ownership of asynchronous reader transitions. A save refresh borrows the
//! current token; only user navigation may advance it.

export type Stale = () => boolean;

let navigationGeneration = 0;

export function currentNavigation(): Stale {
  const generation = navigationGeneration;
  return () => generation !== navigationGeneration;
}

/// Nested selections inherit this token and check it after every await, so the
/// last navigation requested wins, not whichever fetch happens to finish last.
export function beginNavigation(): Stale {
  navigationGeneration += 1;
  return currentNavigation();
}
