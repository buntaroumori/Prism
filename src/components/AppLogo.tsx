import type { JSX } from "react";
import { useId } from "react";

import { cn } from "@/lib/utils";

interface AppLogoProps {
  className?: string;
}

/**
 * The Prism mark: a white-glass triangle refracting light into the spectrum.
 *
 * Pure inline SVG (no external asset, no emoji), gradient ids are namespaced
 * with `useId` so multiple instances never collide.
 */
export function AppLogo({ className }: AppLogoProps): JSX.Element {
  const gradientId = useId();
  const faceId = useId();

  return (
    <svg
      viewBox="0 0 48 48"
      fill="none"
      role="img"
      aria-label="Prism"
      className={cn("h-9 w-9", className)}
    >
      <defs>
        <linearGradient
          id={gradientId}
          x1="24"
          y1="7"
          x2="24"
          y2="41"
          gradientUnits="userSpaceOnUse"
        >
          <stop stopColor="#FFFFFF" stopOpacity="0.95" />
          <stop offset="1" stopColor="#A7B2FF" stopOpacity="0.45" />
        </linearGradient>
        <linearGradient id={faceId} x1="8" y1="14" x2="44" y2="34" gradientUnits="userSpaceOnUse">
          <stop stopColor="#6BD5FF" />
          <stop offset="0.35" stopColor="#B08CFF" />
          <stop offset="0.7" stopColor="#FF6B9A" />
          <stop offset="1" stopColor="#FFB86B" />
        </linearGradient>
      </defs>

      <path
        d="M24 5.5 42.5 39a2 2 0 0 1-1.8 3H7.3a2 2 0 0 1-1.8-3L24 5.5Z"
        stroke={`url(#${gradientId})`}
        strokeWidth="1.6"
        fill="rgba(255,255,255,0.035)"
        strokeLinejoin="round"
      />
      <path
        d="M24 7.5v34"
        stroke="white"
        strokeOpacity="0.22"
        strokeWidth="1"
        strokeLinecap="round"
      />
      <path
        d="M25.5 17.5 45 22.2M25 24l19.5 4.8M24.6 30.6 44 35.4"
        stroke={`url(#${faceId})`}
        strokeWidth="1.7"
        strokeLinecap="round"
      />
      <circle cx="24" cy="24" r="1.6" fill="white" fillOpacity="0.85" />
    </svg>
  );
}
