/** A real DOM settings row measured into the GPUI scrollport. */
let viewport: number[];
const rows = new Set<MeasuredSettingsRow>();
export function placeSettingsRowsViewport(left: number, top: number, width: number, height: number) {
  viewport = [left, top, width, height];
  for (const row of rows) row.place();
}

export class MeasuredSettingsRow {
  readonly element: HTMLDivElement;
  height: number;
  private bounds: number[] | undefined;
  private visible = false;
  private locked = false;

  constructor(id: string, initialHeight: number, changed: () => void) {
    this.height = initialHeight;
    this.element = document.createElement('div');
    this.element.id = id;
    this.element.style.cssText = 'position:fixed;z-index:2;pointer-events:auto;box-sizing:border-box;color:var(--dsw-alias-label-primary);font-family:var(--dsw-font-family);';
    document.getElementById('settings-shell')!.append(this.element);
    rows.add(this);
    new ResizeObserver(() => {
      const measured = this.element.getBoundingClientRect().height;
      if (!this.visible || Math.abs(this.height - measured) < 0.1) return;
      this.height = measured; this.place(); changed();
    }).observe(this.element);
    this.element.addEventListener('focusin', event => {
      if (!viewport || !this.visible || this.locked || !(event.target instanceof HTMLElement)) return;
      const [x, y, w, h] = viewport;
      const rect = event.target.getBoundingClientRect();
      const delta = rect.top < y ? rect.top - y - 8 : rect.bottom > y + h ? rect.bottom - y - h + 8 : 0;
      if (delta) document.querySelector('canvas')?.dispatchEvent(new WheelEvent('wheel', {
        bubbles: true, cancelable: true, clientX: x + w / 2, clientY: y + h / 2, deltaY: delta,
      }));
    });
    this.element.addEventListener('wheel', event => {
      if (!this.visible || this.locked) return;
      event.preventDefault();
      document.querySelector('canvas')?.dispatchEvent(new WheelEvent('wheel', {
        bubbles: true, cancelable: true, clientX: event.clientX, clientY: event.clientY,
        deltaX: event.deltaX, deltaY: event.deltaY, deltaMode: event.deltaMode,
        ctrlKey: event.ctrlKey, metaKey: event.metaKey, altKey: event.altKey, shiftKey: event.shiftKey,
      }));
    }, { passive: false });
  }

  show(visible: boolean, locked: boolean) {
    this.visible = visible; this.locked = locked;
    this.element.hidden = !visible || locked;
    this.element.inert = locked;
    this.place();
  }

  measure(left: number, top: number, width: number) { this.bounds = [left, top, width]; this.place(); }

  place() {
    if (!this.bounds || !viewport) return;
    const [left, top, width] = this.bounds, [x, y, w, h] = viewport;
    Object.assign(this.element.style, { left: `${left}px`, top: `${top}px`, width: `${width}px`,
      clipPath: `inset(${y - top}px ${left + width - x - w}px ${top + this.height - y - h}px ${x - left}px)` });
  }
}
