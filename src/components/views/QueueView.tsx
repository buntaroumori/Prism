import type { JSX } from "react";

import { QueuePanel } from "@/components/player/QueuePanel";
import { useUiStore } from "@/stores/uiStore";

/** Full content-area queue route; Back returns to the actual previous page. */
export function QueueView(): JSX.Element {
  const back = useUiStore((state) => state.back);
  return <QueuePanel onClose={back} />;
}
