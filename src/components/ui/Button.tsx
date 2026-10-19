import type { JSX } from "react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Loader2, type LucideIcon } from "lucide-react";

import { cn } from "@/lib/utils";

type ButtonVariant = "primary" | "secondary" | "ghost";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  icon?: LucideIcon;
  loading?: boolean;
  children: ReactNode;
}

const VARIANT_CLASSES: Record<ButtonVariant, string> = {
  primary:
    "bg-white text-oled-950 hover:bg-white/90 shadow-[0_8px_24px_-12px_rgb(255_255_255/0.5)]",
  secondary: "border border-white/10 bg-white/[0.055] text-white/85 hover:bg-white/[0.085]",
  ghost: "text-white/55 hover:bg-white/[0.06] hover:text-white/85",
};

/**
 * Single button primitive: consistent height, focus ring, loading state and
 * an optional Lucide icon rendered as a real SVG at a fixed optical size.
 */
export function Button({
  variant = "secondary",
  icon: Icon,
  loading = false,
  className,
  children,
  disabled,
  type = "button",
  ...rest
}: ButtonProps): JSX.Element {
  const isDisabled = disabled === true || loading;

  return (
    <button
      type={type}
      disabled={isDisabled}
      className={cn(
        "inline-flex h-9 items-center justify-center gap-2 rounded-full px-4 text-[13px] font-medium tracking-tight transition-colors duration-150 select-none",
        "focus-visible:ring-2 focus-visible:ring-accent/70 focus-visible:ring-offset-2 focus-visible:ring-offset-oled-950 focus-visible:outline-none",
        "disabled:pointer-events-none disabled:opacity-45",
        VARIANT_CLASSES[variant],
        className,
      )}
      {...rest}
    >
      {loading ? (
        <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
      ) : Icon !== undefined ? (
        <Icon className="h-4 w-4" aria-hidden="true" />
      ) : null}
      {children}
    </button>
  );
}
