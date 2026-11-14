import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useRef, useState, type JSX, type ReactNode } from "react";

/** All records are known in advance; only visible card rows are mounted. */
export function VirtualCardGrid<T>({
  items,
  minimum,
  extraHeight,
  render,
}: {
  items: T[];
  minimum: number;
  extraHeight: number;
  render: (item: T, size: number) => ReactNode;
}): JSX.Element {
  const ref = useRef<HTMLDivElement>(null);
  const [layout, setLayout] = useState({ width: 0, top: 0 });
  const getScroller = useCallback(
    () => ref.current?.closest<HTMLElement>("[data-prism-scroll]") ?? null,
    [],
  );
  useEffect(() => {
    const node = ref.current;
    const scroll = getScroller();
    if (node === null || scroll === null) return;
    const measure = (): void => {
      const width = node.clientWidth;
      if (width === 0) return;
      const top =
        node.getBoundingClientRect().top - scroll.getBoundingClientRect().top + scroll.scrollTop;
      setLayout((last) =>
        Math.abs(last.width - width) < 1 && Math.abs(last.top - top) < 1 ? last : { width, top },
      );
    };
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    observer.observe(scroll);
    measure();
    return () => {
      observer.disconnect();
    };
  }, [getScroller]);
  const gap = 20;
  const columns = Math.max(1, Math.floor((layout.width + gap) / (minimum + gap)));
  const size =
    layout.width > 0 ? Math.floor((layout.width - gap * (columns - 1)) / columns) : minimum;
  const rowHeight = size + extraHeight;
  const virtual = useVirtualizer({
    count: Math.ceil(items.length / columns),
    getScrollElement: getScroller,
    estimateSize: () => rowHeight,
    scrollMargin: layout.top,
    overscan: 3,
  });
  useEffect(() => {
    virtual.measure();
  }, [virtual, rowHeight, columns]);
  return (
    <div ref={ref} style={{ height: virtual.getTotalSize(), position: "relative", width: "100%" }}>
      {virtual.getVirtualItems().map((row) => (
        <div
          key={row.key}
          style={{
            position: "absolute",
            top: 0,
            left: 0,
            width: "100%",
            transform: `translateY(${String(row.start - layout.top)}px)`,
            display: "grid",
            gridTemplateColumns: `repeat(${String(columns)}, minmax(0, 1fr))`,
            columnGap: gap,
          }}
        >
          {items
            .slice(row.index * columns, (row.index + 1) * columns)
            .map((item) => render(item, size))}
        </div>
      ))}
    </div>
  );
}
