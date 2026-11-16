const background = new WeakMap();

export function acquireSurfaceBackground(surface, excludedIds = []) {
  const owned = new Set();
  const apply = () => {
    for (const node of document.body.children) {
      if (node === surface || excludedIds.includes(node.id)
          || !(node instanceof HTMLElement) || owned.has(node)) continue;
      let entry = background.get(node);
      if (!entry) {
        entry = { previous: node.inert, count: 0 };
        background.set(node, entry);
      }
      owned.add(node);
      entry.count += 1;
      node.inert = true;
    }
  };
  const observer = new MutationObserver(apply);
  apply();
  observer.observe(document.body, { childList: true });
  return () => {
    observer.disconnect();
    for (const node of owned) {
      const entry = background.get(node);
      entry.count -= 1;
      if (!entry.count) {
        node.inert = entry.previous;
        background.delete(node);
      }
    }
    owned.clear();
  };
}
