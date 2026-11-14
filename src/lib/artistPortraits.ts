import { hasBackend } from "@/lib/cover";
import { libraryApi } from "@/lib/ipc";
import { loadAllPages } from "@/lib/loadAllPages";

// Avoid starting work for cards that scrolled away while waiting in the queue.
// Backend pacing applies across all views, including requests already in flight.
type Portrait = NonNullable<Awaited<ReturnType<typeof libraryApi.artistPortrait>>>;
type Listener = (portrait: Portrait | null) => void;
interface Job {
  id: string;
  listeners: Set<Listener>;
  running: boolean;
}
const jobs = new Map<string, Job>();
const portraits = new Map<string, Portrait>();
const queue: Job[] = [];
let active = 0;

function drain(): void {
  while (active < 3 && queue.length > 0) {
    const job = queue.shift();
    if (job === undefined || job.listeners.size === 0 || jobs.get(job.id) !== job) continue;
    job.running = true;
    active += 1;
    void libraryApi
      .artistPortrait(job.id)
      .catch((error: unknown) => {
        console.warn("Artist portrait lookup failed", error);
        return null;
      })
      .then((portrait) => {
        if (portrait !== null) portraits.set(job.id, portrait);
        for (const listener of job.listeners) listener(portrait);
      })
      .finally(() => {
        active -= 1;
        if (jobs.get(job.id) === job) jobs.delete(job.id);
        drain();
      });
  }
}

export function subscribePortrait(id: string, listener: Listener): () => void {
  let job = jobs.get(id);
  if (job === undefined) {
    job = { id, listeners: new Set(), running: false };
    jobs.set(id, job);
    queue.push(job);
  }
  const selected = job;
  selected.listeners.add(listener);
  drain();
  return () => {
    selected.listeners.delete(listener);
    if (!selected.running && selected.listeners.size === 0 && jobs.get(id) === selected)
      jobs.delete(id);
  };
}

/** Warm the persistent portrait cache in the background, one artist at a time. */
export function warmArtistPortraits(): () => void {
  if (!hasBackend()) return () => undefined;
  let active = true;
  let cancelWait: (() => void) | null = null;
  void loadAllPages(
    (offset, limit) => libraryApi.artists({ offset, limit }),
    () => active,
  )
    .then(async (page) => {
      if (page === null) return;
      for (const artist of page.items) {
        if (!active) break;
        if (portraits.has(artist.id)) continue;
        await new Promise<void>((resolve) => {
          const unsubscribe = subscribePortrait(artist.id, () => {
            cancelWait = null;
            resolve();
          });
          cancelWait = () => {
            unsubscribe();
            resolve();
          };
        });
      }
    })
    .catch((error: unknown) => {
      console.warn("Artist portrait warmup failed", error);
    });
  return () => {
    active = false;
    cancelWait?.();
  };
}

export function cachedPortrait(id: string): Portrait | null {
  return portraits.get(id) ?? null;
}
