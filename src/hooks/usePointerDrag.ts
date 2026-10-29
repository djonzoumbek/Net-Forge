// Drag and drop with pointer events, not the HTML5 drag API: in Tauri the
// webview's native drag-and-drop handling (used for file drops) interferes with
// HTML5 drags on Linux, and pointer events also give full control of the visuals.
import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

/** Distance (px) before a press becomes a drag: below it, it stays a click. */
const DRAG_THRESHOLD = 5;

export interface DragState<T, D> {
  item: T;
  x: number;
  y: number;
  /** Where the item would land if released now; `null`: nowhere valid. */
  drop: D | null;
}

interface PointerDragOptions<T, D> {
  /** Resolves the drop target under the pointer (`element` is the topmost one). */
  findDrop: (element: Element | null, item: T, x: number, y: number) => D | null;
  onDrop: (item: T, drop: D) => void;
}

export function usePointerDrag<T, D>({ findDrop, onDrop }: PointerDragOptions<T, D>) {
  const [drag, setDrag] = useState<DragState<T, D> | null>(null);
  // Window listeners outlive renders: they read the latest callbacks here.
  const options = useRef({ findDrop, onDrop });
  useEffect(() => {
    options.current = { findDrop, onDrop };
  });
  const cleanup = useRef<(() => void) | null>(null);
  useEffect(() => () => cleanup.current?.(), []);

  function startDrag(event: ReactPointerEvent, item: T) {
    if (event.button !== 0) return;
    cleanup.current?.();
    const startX = event.clientX;
    const startY = event.clientY;
    let current: DragState<T, D> | null = null;

    const move = (e: PointerEvent) => {
      if (!current && Math.hypot(e.clientX - startX, e.clientY - startY) < DRAG_THRESHOLD) return;
      const element = document.elementFromPoint(e.clientX, e.clientY);
      current = { item, x: e.clientX, y: e.clientY, drop: options.current.findDrop(element, item, e.clientX, e.clientY) };
      document.body.classList.add("dragging");
      setDrag(current);
    };
    const up = () => {
      const finished = current;
      stop();
      if (!finished) return;
      // The release also produces a click on the element under the pointer:
      // swallow it, a drop is not a click.
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
      if (finished.drop !== null) options.current.onDrop(finished.item, finished.drop);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") stop();
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", stop);
      window.removeEventListener("keydown", key);
      document.body.classList.remove("dragging");
      cleanup.current = null;
      current = null;
      setDrag(null);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", stop);
    window.addEventListener("keydown", key);
    cleanup.current = stop;
  }

  return { drag, startDrag };
}

function swallow(event: MouseEvent) {
  event.stopPropagation();
  event.preventDefault();
}
