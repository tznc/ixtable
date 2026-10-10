import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { EllipsisVertical } from "lucide-react";

/** A datasheet column header's menu: sort, hide, and freeze. */
export function ColumnMenu({
  name,
  frozen,
  canHide,
  onSort,
  onHide,
  onFreeze,
}: {
  name: string;
  frozen: boolean;
  canHide: boolean;
  onSort: (descending: boolean) => void;
  onHide: () => void;
  onFreeze: () => void;
}) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button type="button" className="column-menu" aria-label={`Column options for ${name}`}>
          <EllipsisVertical aria-hidden />
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="view-menu" sideOffset={4} align="start">
          <DropdownMenu.Item onSelect={() => onSort(false)}>Sort ascending</DropdownMenu.Item>
          <DropdownMenu.Item onSelect={() => onSort(true)}>Sort descending</DropdownMenu.Item>
          <DropdownMenu.Item disabled={!canHide} onSelect={onHide}>
            Hide column
          </DropdownMenu.Item>
          <DropdownMenu.Item onSelect={onFreeze}>
            {frozen ? "Unfreeze columns" : "Freeze through this column"}
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
