/** Read the complete ordered catalogue up front; scrolling never triggers I/O. */
export async function loadAllPages<T>(
  fetch: (offset: number, limit: number) => Promise<{ items: T[]; total: number }>,
  active: () => boolean,
): Promise<{ items: T[]; total: number } | null> {
  const limit = 500;
  const first = await fetch(0, limit);
  if (!active()) return null;
  const pages: T[][] = [first.items];
  const count = Math.ceil(first.total / limit);
  for (let index = 1; index < count; index += 4) {
    const requests = Array.from({ length: Math.min(4, count - index) }, (_, i) =>
      fetch((index + i) * limit, limit),
    );
    const batch = await Promise.all(requests);
    if (!active()) return null;
    pages.push(...batch.map((page) => page.items));
  }
  const items = pages.flat();
  if (items.length !== first.total) throw new Error("Library changed while loading; please retry.");
  return { items, total: first.total };
}
