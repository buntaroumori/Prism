import { useEffect, useState, type JSX, type SyntheticEvent } from "react";
import { createPortal } from "react-dom";

import { Button } from "@/components/ui/Button";
import { usePlaylistStore } from "@/stores/playlistStore";
import type { PlaylistRow } from "@/types/ipc";

interface PlaylistEditorDialogProps {
  playlist: PlaylistRow;
  /** Context-menu rename intentionally leaves description and genre untouched. */
  titleOnly?: boolean;
  onClose: () => void;
  onSaved?: (playlist: PlaylistRow) => void;
}

/** Title-only rename or the complete in-playlist metadata editor. */
export function PlaylistEditorDialog({
  playlist,
  titleOnly = false,
  onClose,
  onSaved,
}: PlaylistEditorDialogProps): JSX.Element {
  const update = usePlaylistStore((state) => state.update);
  const [name, setName] = useState(playlist.name);
  const [description, setDescription] = useState(playlist.description ?? "");
  const [genre, setGenre] = useState(playlist.genre ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [busy, onClose]);

  const save = async (event: SyntheticEvent): Promise<void> => {
    event.preventDefault();
    if (!name.trim() || busy) return;
    setBusy(true);
    setError(null);
    const updated = await update(playlist.id, {
      name,
      description: titleOnly ? playlist.description : description.trim() || null,
      genre: titleOnly ? playlist.genre : genre.trim() || null,
    });
    setBusy(false);
    if (updated === null) {
      setError(usePlaylistStore.getState().error ?? "The playlist could not be updated.");
      return;
    }
    onSaved?.(updated);
    onClose();
  };

  return createPortal(
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 p-6"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget && !busy) onClose();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={titleOnly ? "Rename playlist" : "Edit playlist"}
        className="glass w-full max-w-[520px] rounded-3xl border-white/[0.1] bg-oled-950/95 p-6 shadow-2xl"
      >
        <h2 className="text-[20px] font-semibold tracking-[-0.02em] text-white">
          {titleOnly ? "Rename playlist" : "Edit playlist"}
        </h2>
        <p className="mt-1 text-[12px] text-white/35">
          {titleOnly
            ? "Change only the playlist title."
            : "Title, description and genre are saved with this playlist."}
        </p>
        <form onSubmit={(event) => void save(event)} className="mt-5 space-y-4">
          <label className="block">
            <span className="mb-1.5 block text-[11px] font-medium text-white/50">Title</span>
            <input
              autoFocus
              value={name}
              onChange={(event) => {
                setName(event.target.value);
              }}
              maxLength={128}
              aria-label="Playlist title"
              placeholder="Playlist title"
              className="h-10 w-full rounded-xl border border-white/10 bg-white/[0.04] px-3 text-[13px] text-white placeholder:text-white/25 focus:border-accent/40 focus:outline-none"
            />
          </label>

          {!titleOnly ? (
            <>
              <label className="block">
                <span className="mb-1.5 block text-[11px] font-medium text-white/50">
                  Description <span className="font-normal text-white/25">(optional)</span>
                </span>
                <textarea
                  value={description}
                  onChange={(event) => {
                    setDescription(event.target.value);
                  }}
                  maxLength={2_000}
                  rows={3}
                  aria-label="Playlist description"
                  placeholder="What belongs in this playlist?"
                  className="w-full resize-none rounded-xl border border-white/10 bg-white/[0.04] px-3 py-2.5 text-[13px] text-white placeholder:text-white/25 focus:border-accent/40 focus:outline-none"
                />
              </label>
              <label className="block">
                <span className="mb-1.5 block text-[11px] font-medium text-white/50">
                  Genre <span className="font-normal text-white/25">(optional)</span>
                </span>
                <input
                  value={genre}
                  onChange={(event) => {
                    setGenre(event.target.value);
                  }}
                  maxLength={128}
                  aria-label="Playlist genre"
                  placeholder="Playlist genre"
                  className="h-10 w-full rounded-xl border border-white/10 bg-white/[0.04] px-3 text-[13px] text-white placeholder:text-white/25 focus:border-accent/40 focus:outline-none"
                />
              </label>
            </>
          ) : null}

          {error !== null ? (
            <p role="alert" className="text-[11.5px] text-critical">
              {error}
            </p>
          ) : null}
          <div className="flex justify-end gap-2 pt-1">
            <Button variant="ghost" disabled={busy} onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" loading={busy} disabled={!name.trim()}>
              Save
            </Button>
          </div>
        </form>
      </div>
    </div>,
    document.body,
  );
}
