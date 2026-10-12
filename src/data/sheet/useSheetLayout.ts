import { useMemo } from "react";
import { useDocumentConfig } from "../../lib/config-store";
import { type DatasheetLayout, readLayout, writeLayout } from "./layout";

/** `table`'s saved datasheet layout and a setter that records one undo step per change. */
export function useSheetLayout(table: string, onError: (message: string) => void) {
  const { config, update } = useDocumentConfig();
  const layout = useMemo(() => readLayout(config, table), [config, table]);
  const save = (next: DatasheetLayout, label: string) =>
    update((draft) => writeLayout(draft, table, next), label).catch((e: unknown) =>
      onError(e instanceof Error ? e.message : String(e)),
    );
  return [layout, save] as const;
}
