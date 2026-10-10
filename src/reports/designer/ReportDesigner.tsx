import {
  Calculator,
  FileStack,
  Image,
  Minus,
  Square,
  Table2,
  TextCursorInput,
  Type,
} from "lucide-react";
import { useEffect, useState } from "react";
import type { SavedQuery } from "../../query/types";
import { type AssetSummary, listAssets } from "../api";
import {
  type BandKey,
  bandEntries,
  contentWidth,
  embeddableReports,
  findComponent,
  getBand,
  isPageBand,
  newComponent,
  reportProblems,
  withBand,
} from "../model";
import type { ComponentKind, Report, ReportComponent } from "../types";
import { BandCanvas } from "./BandCanvas";
import { bandMinHeight, clampGeometry } from "./geometry";
import { ComponentProperties } from "./ComponentProperties";
import { BandProperties, ReportSettings } from "./ReportSettings";

const PALETTE: [ComponentKind, string, typeof Type][] = [
  ["staticText", "Add text", Type],
  ["field", "Add field", TextCursorInput],
  ["calculated", "Add calculated", Calculator],
  ["image", "Add image", Image],
  ["line", "Add line", Minus],
  ["rectangle", "Add rectangle", Square],
  ["table", "Add table", Table2],
  ["subreport", "Add subreport", FileStack],
];

/** Kinds that page headers and footers can't hold. */
const BODY_ONLY: ComponentKind[] = ["table", "subreport"];

type Change = (fn: (report: Report) => Report, label?: string) => void;

/** Band editor canvas with palette and properties panel. */
export function ReportDesigner({
  report,
  reports = [],
  queries,
  columns,
  change,
  focusId,
}: {
  report: Report;
  /** Every report of the document, for subreports. */
  reports?: Report[];
  queries: SavedQuery[];
  columns: string[];
  change: Change;
  /** Component to select on mount (a Problems link). */
  focusId?: string;
}) {
  const [bandKey, setBandKey] = useState<BandKey>(
    () => (focusId && findComponent(report, focusId)?.key) || "detail",
  );
  const [selectedId, setSelectedId] = useState<string | null>(focusId ?? null);
  const [assets, setAssets] = useState<AssetSummary[]>([]);
  const width = contentWidth(report.page);
  const bands = bandEntries(report);
  const activeKey = getBand(report, bandKey) ? bandKey : "detail";
  const active = bands.find((b) => b.key === activeKey) ?? bands[0];
  const selected = selectedId ? findComponent(report, selectedId) : undefined;
  const problems = reportProblems(report, reports);

  useEffect(() => {
    listAssets()
      .then(setAssets)
      .catch(() => setAssets([]));
  }, []);

  const updateComponent = (
    id: string,
    label: string,
    fn: (c: ReportComponent) => ReportComponent,
  ) =>
    change((r) => {
      const found = findComponent(r, id);
      if (!found) return r;
      return withBand(r, found.key, (band) => {
        const components = band.components.map((c) => (c.id === id ? fn(c) : c));
        const bottom = Math.max(band.height, ...components.map((c) => c.y + c.h));
        return { ...band, components, height: bottom };
      });
    }, label);

  const remove = (id: string) => {
    const found = findComponent(report, id);
    if (!found) return;
    setSelectedId(null);
    change(
      (r) =>
        withBand(r, found.key, (band) => ({
          ...band,
          components: band.components.filter((c) => c.id !== id),
        })),
      "Delete report component",
    );
  };

  const pageBand = isPageBand(active.key);
  const add = (kind: ComponentKind) => {
    const band = active.band;
    const right = Math.max(0, ...band.components.map((c) => c.x + c.w));
    const component = newComponent(kind, width, { x: right, y: 0 });
    if (component.x + component.w > width || component.x < right) component.x = 0;
    setSelectedId(component.id);
    change(
      (r) =>
        withBand(r, active.key, (b) => ({
          ...b,
          height: Math.max(b.height, component.y + component.h),
          components: [...b.components, component],
        })),
      `Add ${kind}`,
    );
  };

  return (
    <div className="report-designer">
      <div className="report-canvas-area">
        <div className="report-palette" role="toolbar" aria-label="Report components">
          {PALETTE.map(([kind, label, Icon]) => {
            const blocked = pageBand && BODY_ONLY.includes(kind);
            return (
              <button
                key={kind}
                type="button"
                aria-disabled={blocked ? "true" : undefined}
                aria-describedby={blocked ? "report-table-hint" : undefined}
                onClick={() => {
                  if (!blocked) add(kind);
                }}
              >
                <Icon /> {label}
              </button>
            );
          })}
        </div>
        {pageBand && (
          <p className="report-hint" id="report-table-hint">
            Tables and subreports are not supported in page headers or footers.
          </p>
        )}
        <p className="report-hint" id="report-canvas-hint">
          Adding to: <b>{active.label}</b>. Click a band label to choose it. Drag components to move
          them and the corner handle to resize. With a component focused, arrow keys move it 1 pt
          (Shift: 10 pt), Alt+arrow keys resize it, and Delete removes it.
        </p>
        {problems.length > 0 && (
          <ul className="report-diagnostic" role="alert" aria-label="Report problems">
            {problems.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        )}
        <BandCanvas
          bands={bands}
          width={width}
          selectedBand={activeKey}
          selectedId={selected ? selectedId : null}
          onSelectBand={(key) => {
            setBandKey(key);
            setSelectedId(null);
          }}
          onSelect={(id, key) => {
            setSelectedId(id);
            setBandKey(key);
          }}
          onGeometry={(id, geometry) =>
            updateComponent(id, "Move report component", (c) => ({ ...c, ...geometry }))
          }
          onBandHeight={(key, height) =>
            change((r) => withBand(r, key, (b) => ({ ...b, height })), "Resize band")
          }
          onDelete={remove}
        />
      </div>
      <aside className="report-panel" aria-label="Report properties">
        <small>PROPERTIES</small>
        {selected ? (
          <ComponentProperties
            component={selected.component}
            pageBand={isPageBand(selected.key)}
            columns={columns}
            queries={queries}
            assets={assets}
            reports={reports}
            subreports={embeddableReports(reports, report)}
            onDelete={() => remove(selected.component.id)}
            onChange={(patch) =>
              updateComponent(selected.component.id, "Edit report component", (c) => {
                const next = { ...c, ...patch } as ReportComponent;
                const geometry =
                  "x" in patch || "y" in patch || "w" in patch || "h" in patch
                    ? clampGeometry(next, width, Number.POSITIVE_INFINITY)
                    : {};
                return { ...next, ...geometry };
              })
            }
          />
        ) : (
          <>
            <BandProperties
              label={active.label}
              band={active.band}
              minHeight={bandMinHeight(active)}
              pageBand={isPageBand(active.key)}
              onChange={(patch) =>
                change((r) => withBand(r, active.key, (b) => ({ ...b, ...patch })), "Edit band")
              }
            />
            <ReportSettings report={report} queries={queries} columns={columns} change={change} />
          </>
        )}
      </aside>
    </div>
  );
}
