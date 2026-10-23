import { open } from "@tauri-apps/plugin-dialog";

/** Native image picker shared by the playlist page and its context menu. */
export async function pickPlaylistCover(): Promise<string | null> {
  const selected = await open({
    title: "Choose playlist cover",
    multiple: false,
    directory: false,
    filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png", "webp"] }],
  });
  return typeof selected === "string" ? selected : null;
}
