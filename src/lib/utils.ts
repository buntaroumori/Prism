import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/**
 * Compose Tailwind classes with conflict resolution.
 *
 * `tailwind-merge` understands the v4 token set declared in
 * `tailwind.config.ts`, so conditional variants never leave two competing
 * utilities (e.g. `px-4` + `px-6`) in the DOM.
 */
export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
