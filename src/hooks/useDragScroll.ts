import { useCallback, useEffect, useRef, useState, type RefObject } from "react";

interface DragScroll {
  /** Attach to the scrolling element. */
  ref: RefObject<HTMLDivElement | null>;
  /** Spread on the same element as `ref`. */
  handlers: {
    onPointerDown: (event: React.PointerEvent<HTMLDivElement>) => void;
    onPointerMove: (event: React.PointerEvent<HTMLDivElement>) => void;
    onPointerUp: (event: React.PointerEvent<HTMLDivElement>) => void;
    onPointerCancel: (event: React.PointerEvent<HTMLDivElement>) => void;
    onClickCapture: (event: React.MouseEvent<HTMLDivElement>) => void;
  };
  /** True when the gesture that just ended was a drag, not a click. */
  wasDragged: () => boolean;
  /** Scroll by most of one viewport, smoothly. */
  scrollBy: (direction: 1 | -1) => void;
  canScrollLeft: boolean;
  canScrollRight: boolean;
}

/**
 * Pointer-drag horizontal scrolling for a rail.
 *
 * A mouse cannot scroll a horizontal list the way a trackpad can, and a rail
 * that only answers shift+wheel reads as broken. This applies the behaviour
 * every media rail has: press, move, release — with inertia, and with click
 * suppression so releasing over a card does not navigate.
 *
 * Touch and pen are left to the browser: they already scroll natively, and
 * intercepting them only fights the platform.
 */
export function useDragScroll(): DragScroll {
  const ref = useRef<HTMLDivElement | null>(null);
  const pressed = useRef(false);
  const dragging = useRef(false);
  /** Set by `finish` when the gesture moved: the click that follows is fake. */
  const suppressClick = useRef(false);
  const moved = useRef(0);
  const startX = useRef(0);
  const startScroll = useRef(0);
  const velocity = useRef(0);
  const lastX = useRef(0);
  const lastT = useRef(0);
  const frame = useRef<number | null>(null);

  const [left, setLeft] = useState(false);
  const [right, setRight] = useState(false);

  const stopInertia = useCallback(() => {
    if (frame.current !== null) {
      cancelAnimationFrame(frame.current);
      frame.current = null;
    }
  }, []);

  const glide = useCallback(() => {
    const node = ref.current;
    if (node === null) {
      return;
    }
    velocity.current *= 0.92;
    if (Math.abs(velocity.current) < 0.05) {
      frame.current = null;
      return;
    }
    node.scrollLeft -= velocity.current * 16;
    frame.current = requestAnimationFrame(glide);
  }, []);

  const measure = useCallback(() => {
    const node = ref.current;
    if (node === null) {
      return;
    }
    const max = node.scrollWidth - node.clientWidth;
    setLeft(node.scrollLeft > 1);
    setRight(max > 1 && node.scrollLeft < max - 1);
  }, []);

  useEffect(() => {
    const node = ref.current;
    if (node === null) {
      return;
    }
    node.addEventListener("scroll", measure, { passive: true });
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    for (const child of Array.from(node.children)) {
      observer.observe(child);
    }
    measure();
    return () => {
      node.removeEventListener("scroll", measure);
      observer.disconnect();
      stopInertia();
    };
  }, [measure, stopInertia]);

  const onPointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.pointerType === "touch" || event.button !== 0) {
        return;
      }
      const node = ref.current;
      if (node === null) {
        return;
      }
      stopInertia();
      // The pointer is deliberately *not* captured here.
      //
      // Capturing on press retargets the subsequent `click` to the capturing
      // element — the rail — so pressing a card and releasing it over the same
      // card delivered the click to the rail instead of the button inside it.
      // Every tile in every rail became inert: no album, no artist, nothing.
      // Capture starts with the movement, below, where the gesture really
      // becomes a drag.
      dragging.current = false;
      pressed.current = true;
      suppressClick.current = false;
      moved.current = 0;
      startX.current = event.clientX;
      lastX.current = event.clientX;
      lastT.current = performance.now();
      startScroll.current = node.scrollLeft;
      velocity.current = 0;
    },
    [stopInertia],
  );

  const onPointerMove = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    const node = ref.current;
    if (!pressed.current || node === null) {
      return;
    }
    const delta = event.clientX - startX.current;
    if (!dragging.current) {
      if (Math.abs(delta) <= 4) {
        return;
      }
      // A real drag: from here the gesture owns the pointer, and the click it
      // produces on release must be swallowed.
      dragging.current = true;
      event.currentTarget.setPointerCapture(event.pointerId);
    }
    if (Math.abs(delta) > Math.abs(moved.current)) {
      moved.current = delta;
    }
    node.scrollLeft = startScroll.current - delta;
    const now = performance.now();
    const elapsed = Math.max(1, now - lastT.current);
    velocity.current = (event.clientX - lastX.current) / elapsed;
    lastX.current = event.clientX;
    lastT.current = now;
  }, []);

  const finish = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (!pressed.current) {
        return;
      }
      pressed.current = false;
      // Only a gesture that actually moved suppresses the click; a plain press
      // and release must reach the card, or the rail is decoration.
      suppressClick.current = dragging.current && Math.abs(moved.current) > 4;
      dragging.current = false;
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
      if (Math.abs(velocity.current) > 0.02) {
        stopInertia();
        frame.current = requestAnimationFrame(glide);
      }
      measure();
    },
    [glide, measure, stopInertia],
  );

  return {
    ref,
    handlers: {
      onPointerDown,
      onPointerMove,
      // A drag that ends outside the rail still has to clean up, so the
      // pointer-capture release happens in `finish`, not in a click handler.
      onPointerUp: finish,
      onPointerCancel: finish,
      // Capture phase: the card's own handler must never see the click. It runs
      // once, for the release that ended the drag, and is cleared on the next
      // press.
      onClickCapture: (event) => {
        if (suppressClick.current) {
          suppressClick.current = false;
          event.preventDefault();
          event.stopPropagation();
        }
      },
    },
    wasDragged: () => Math.abs(moved.current) > 4,
    scrollBy: (direction) => {
      const node = ref.current;
      if (node === null) {
        return;
      }
      stopInertia();
      node.scrollBy({ left: direction * node.clientWidth * 0.82, behavior: "smooth" });
    },
    canScrollLeft: left,
    canScrollRight: right,
  };
}
