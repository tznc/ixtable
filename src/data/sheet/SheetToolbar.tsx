import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { EyeOff, Filter as FilterIcon, FilterX, Search, Sigma, Snowflake, X } from "lucide-react";
import type { Filter } from "../../lib/types";
import { describeFilter } from "./filters";

/** Datasheet commands (Access's Sort & Filter and Records groups) and the active filter chips. */
export function SheetToolbar({
  canFilter,
  onFilterSelection,
  filters,
  onRemoveFilter,
  onClearFilters,
  onFind,
  totalsShown,
  onToggleTotals,
  hidden,
  onShow,
  frozen,
  onUnfreeze,
}: {
  /** A filterable cell is selected. */
  canFilter: boolean;
  onFilterSelection: (exclude: boolean) => void;
  filters: Filter[];
  onRemoveFilter: (index: number) => void;
  onClearFilters: () => void;
  onFind: () => void;
  totalsShown: boolean;
  onToggleTotals: () => void;
  hidden: string[];
  /** Shows the named hidden columns. */
  onShow: (columns: string[]) => void;
  frozen: number;
  onUnfreeze: () => void;
}) {
  return (
    <div className="sheet-toolbar" role="toolbar" aria-label="Datasheet tools">
      <button
        type="button"
        disabled={!canFilter}
        title="Show only records with the selected value"
        onClick={() => onFilterSelection(false)}
      >
        <FilterIcon aria-hidden />
        Filter by selection
      </button>
      <button
        type="button"
        disabled={!canFilter}
        title="Hide records with the selected value"
        onClick={() => onFilterSelection(true)}
      >
        <FilterX aria-hidden />
        Filter excluding selection
      </button>
      <button type="button" onClick={onFind} title="Find and replace (Ctrl+F)">
        <Search aria-hidden />
        Find
      </button>
      <button type="button" aria-pressed={totalsShown} onClick={onToggleTotals}>
        <Sigma aria-hidden />
        Totals
      </button>
      {hidden.length > 0 && (
        <DropdownMenu.Root>
          <DropdownMenu.Trigger asChild>
            <button type="button">
              <EyeOff aria-hidden />
              {hidden.length} hidden
            </button>
          </DropdownMenu.Trigger>
          <DropdownMenu.Portal>
            <DropdownMenu.Content className="view-menu" sideOffset={6} align="start">
              {hidden.map((name) => (
                <DropdownMenu.Item key={name} onSelect={() => onShow([name])}>
                  Show {name}
                </DropdownMenu.Item>
              ))}
              <DropdownMenu.Item onSelect={() => onShow(hidden)}>
                Show all columns
              </DropdownMenu.Item>
            </DropdownMenu.Content>
          </DropdownMenu.Portal>
        </DropdownMenu.Root>
      )}
      {frozen > 0 && (
        <button type="button" onClick={onUnfreeze}>
          <Snowflake aria-hidden />
          Unfreeze columns
        </button>
      )}
      {filters.length > 0 && (
        <ul className="filter-chips" aria-label="Active filters">
          {filters.map((filter, i) => {
            const label = describeFilter(filter);
            return (
              <li key={`${label}-${i}`}>
                {label}
                <button
                  type="button"
                  aria-label={`Remove filter ${label}`}
                  onClick={() => onRemoveFilter(i)}
                >
                  <X aria-hidden />
                </button>
              </li>
            );
          })}
          <li>
            <button type="button" onClick={onClearFilters}>
              Clear filters
            </button>
          </li>
        </ul>
      )}
    </div>
  );
}
