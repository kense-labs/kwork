// Project painted GPUI controls into the webview's native accessibility tree.
// Physical hit testing and the existing GPUI keyboard order stay with the canvas.
type Rect = [number, number, number, number];
type Action = { action: string; value: string };
type Layout = Action & { bounds: Rect; viewport: Rect; label: string;
  selected?: boolean; expanded?: boolean; hidden: boolean };
type Control = { node: HTMLButtonElement; action: Action; seen: number };
const controls = new Map<string, Control>();
const groups = new Map<string, HTMLElement>();
let generation = 0;
let sweep = 0;
let notify: (action: string, value: string) => void;
const restoreShell = () => document.querySelector<HTMLElement>('#gpui-text-input')?.focus({ preventScroll: true });

export function listenTrajectoryAccessibility(callback: typeof notify) { notify = callback; }
export function beginTrajectoryAccessibilityFrames() {
  generation++;
  if (sweep) return;
  sweep = requestAnimationFrame(() => {
    sweep = 0;
    for (const [key, control] of controls) {
      if (control.seen !== generation) { control.node.remove(); controls.delete(key); }
    }
    for (const [key, group] of groups) {
      if (!group.children.length) { group.remove(); groups.delete(key); continue; }
      // Virtualized rows are inserted as they enter the viewport. Keep the
      // native reading order aligned with their measured canvas positions.
      const ordered = [...group.children].sort((a, b) => {
        const ar = a.getBoundingClientRect(), br = b.getBoundingClientRect();
        return ar.top - br.top || ar.left - br.left;
      });
      ordered.forEach((node, index) => {
        if (group.children[index] !== node) group.insertBefore(node, group.children[index] ?? null);
      });
    }
  });
}
function groupFor(action: string) {
  const key = action === 'view' ? 'view' : action === 'detail-tab' ? 'detail' : 'controls';
  let group = groups.get(key);
  if (!group) {
    group = document.createElement('div');
    group.dataset.trajectoryControl = '';
    group.dataset.trajectoryAccessibility = key;
    group.setAttribute('role', key === 'controls' ? 'group' : 'tablist');
    group.setAttribute('aria-label', key === 'view' ? '会话视图' : key === 'detail' ? '事件详情' : '轨迹记录与操作');
    groups.set(key, group); document.body.append(group);
  }
  return group;
}
export function stageTrajectoryAccessibility(key: string, layout: Layout) {
  const [x, y, right, bottom] = layout.bounds;
  const [vx, vy, vr, vb] = layout.viewport;
  const left = Math.max(x, vx), top = Math.max(y, vy);
  const width = Math.max(0, Math.min(right, vr) - left);
  const height = Math.max(0, Math.min(bottom, vb) - top);
  let control = controls.get(key);
  if (layout.hidden || !width || !height) {
    if (control) { control.node.remove(); controls.delete(key); }
    return;
  }
  if (!control) {
    const node = document.createElement('button');
    node.type = 'button'; node.tabIndex = -1;
    // WebKit removes AXPress when the button has pointer-events:none. Clip
    // its painted/hit-tested area instead, as with the workspace AX surface.
    node.style.cssText = 'position:fixed;z-index:17;pointer-events:auto;clip-path:inset(50%);opacity:0;margin:0;padding:0;border:0;overflow:hidden';
    control = { node, action: layout, seen: generation };
    const current = control;
    node.onclick = () => {
      if (node.closest('[inert]') || !node.isConnected) return;
      restoreShell();
      notify?.('accessible', JSON.stringify(current.action));
    };
    node.onkeydown = event => {
      if (event.isComposing || event.ctrlKey || event.metaKey || event.altKey) return;
      if (event.key === 'Escape') {
        event.preventDefault(); event.stopPropagation(); restoreShell(); notify?.('close', '');
      } else if (event.key === 'Tab') {
        event.preventDefault(); event.stopPropagation(); restoreShell();
        notify?.('accessible-tab', JSON.stringify({ ...current.action, backward: event.shiftKey }));
      }
    };
    controls.set(key, control); groupFor(layout.action).append(node);
  }
  control.seen = generation;
  control.action = { action: layout.action, value: layout.value };
  const { node } = control;
  node.setAttribute('aria-label', layout.label);
  node.setAttribute('role', ['view', 'detail-tab'].includes(layout.action) ? 'tab' : 'button');
  const selected = node.getAttribute('role') === 'tab' ? 'aria-selected' : 'aria-current';
  if (layout.selected != null) node.setAttribute(selected, String(layout.selected));
  else node.removeAttribute(selected);
  if (layout.expanded != null) node.setAttribute('aria-expanded', String(layout.expanded));
  else node.removeAttribute('aria-expanded');
  Object.assign(node.style, { left: `${left}px`, top: `${top}px`, width: `${width}px`, height: `${height}px` });
}
