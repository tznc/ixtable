import { type ComponentType, createElement, useContext, useEffect, useRef, useState } from "react";
import { NAVIGATE_EVENT, provideAppState, SET_STATE_EVENT } from "../automation/context";
import * as dashboardsModule from "../dashboards";
import { GenerateAppButton } from "../design/GenerateAppButton";
import { flattenNavigation, formTable, type NavigationItem } from "../design/schema";
import { useDocumentConfig } from "../lib/config-store";
import type { DocumentConfig } from "../lib/types";
import { ReportRun } from "../reports";
import { ShellContext } from "../shell/context";
import { type BackLink, BackCrumb } from "./BackCrumb";
import { tableContext } from "./data";
import { FormRenderer } from "./FormRenderer";
import {
  canOpen,
  pageFor,
  pageTitle,
  RuntimeContext,
  type RuntimeNavigation,
  type RuntimePage,
  startPage,
  useRuntimeNavigation,
  useRuntimeState,
  visibleNavigation,
} from "./navigation";
import { PopupHost } from "./PopupHost";
import { assignedRuntimeRole } from "./rbac";
import { tableForms } from "./registry";
import "./runtime.css";

type Embeddable = ComponentType<Record<string, unknown>>;
const exported = (module: unknown, name: string) =>
  (module as Record<string, unknown>)[name] as Embeddable | undefined;

/** Renders a component another feature exports (reports, dashboards) when it exists. */
function Embedded({
  module,
  name,
  missing,
  ...props
}: { module: unknown; name: string; missing: string } & Record<string, unknown>) {
  const component = exported(module, name);
  return component ? createElement(component, props) : <p className="rt-muted">{missing}</p>;
}

/** Run mode: the application as its users see it, with navigation, start page, and role preview. */
export function RunMode() {
  const { config } = useDocumentConfig();
  const runtime = useRuntimeState(config);
  // Runtime-only windows (bundles, cloud installations) show the app, not Studio chrome.
  const runtimeOnly = useContext(ShellContext)?.doc.runtimeOnly ?? false;
  const empty = !runtimeOnly && !hasPages(config);
  // An empty app's start page is the blank starter form: the build hint replaces it.
  const page = runtime.page ?? (empty ? null : startPage(config, runtime.roleId));
  const navigation = visibleNavigation(config.design?.navigation ?? [], config, runtime.roleId);
  const roles = config.roles ?? [];
  const assigned = assignedRuntimeRole();
  const back = usePageBack(runtime);
  useActionEvents(runtime);
  return (
    <RuntimeContext.Provider value={runtime}>
      <header className="titlebar">
        <div>
          {!runtimeOnly && <p>PROJECT / RUNTIME · {config.name}</p>}
          <h1>{runtimeOnly ? config.name : "Runtime"}</h1>
        </div>
        <div className="header-actions">
          {assigned ? (
            <span className="rt-role">Role: {assigned.name}</span>
          ) : (
            !runtimeOnly && (
              <label className="rt-role">
                Preview as role
                <select
                  value={runtime.roleId ?? ""}
                  onChange={(e) => runtime.setRoleId(e.target.value || null)}
                >
                  <option value="">Developer (full access)</option>
                  {roles.map((role) => (
                    <option key={role.id} value={role.id}>
                      {role.name}
                    </option>
                  ))}
                </select>
              </label>
            )
          )}
        </div>
      </header>
      <PopupHost onCloseWithoutPopup={() => runtime.canGoBack && runtime.back()} />
      <div className="rt-app">
        <nav className="rt-nav" aria-label="Application navigation">
          {navigation.length ? (
            <NavList items={navigation} active={page?.navId} />
          ) : (
            <p className="rt-muted">Nothing to show for this role.</p>
          )}
        </nav>
        <section className="rt-page" aria-label="Application page">
          {runtime.notice && (
            <p
              className={runtime.notice.tone === "error" ? "rt-error" : "rt-status"}
              role={runtime.notice.tone === "error" ? "alert" : "status"}
            >
              {runtime.notice.message}
            </p>
          )}
          {empty && (
            <div className="rt-empty" role="region" aria-label="Build your app">
              <p className="rt-muted">This application has no pages yet.</p>
              <GenerateAppButton
                className="rt-button primary"
                onDone={(added) =>
                  !added && runtime.notify("There are no tables to generate from.", "error")
                }
              />
            </div>
          )}
          {page ? (
            <PageView
              key={`${runtime.roleId ?? ""}:${runtime.visit}:${JSON.stringify(page)}`}
              page={page}
              back={back}
            />
          ) : (
            runtimeOnly && <p className="rt-muted">This application has no pages yet.</p>
          )}
        </section>
      </div>
    </RuntimeContext.Provider>
  );
}

/**
 * Follows navigation and app-state changes from actions that run outside a page
 * (sync triggers, custom concurrency actions), and gives them the app state.
 */
function useActionEvents(runtime: RuntimeNavigation) {
  const ref = useRef(runtime);
  useEffect(() => {
    ref.current = runtime;
  });
  useEffect(() => {
    const navigate = (event: Event) => {
      const target = (event as CustomEvent<RuntimePage>).detail;
      if (!target?.id) return;
      event.preventDefault();
      ref.current.navigate({ ...target, navId: undefined });
    };
    const setState = (event: Event) => {
      const { scope, key, value } = (event as CustomEvent).detail ?? {};
      if (scope !== "app" || !key) return;
      event.preventDefault();
      ref.current.setAppState(key, value);
    };
    window.addEventListener(NAVIGATE_EVENT, navigate);
    window.addEventListener(SET_STATE_EVENT, setState);
    const release = provideAppState(() => ref.current.app);
    return () => {
      window.removeEventListener(NAVIGATE_EVENT, navigate);
      window.removeEventListener(SET_STATE_EVENT, setState);
      release();
    };
  }, []);
}

/**
 * True when the application has a navigation item of any kind (form, table, dashboard,
 * report, …) other than a new document's blank form (no source, no controls).
 */
function hasPages(config: DocumentConfig) {
  const forms = config.design?.forms ?? [];
  return flattenNavigation(config.design?.navigation ?? []).some((item) => {
    if (item.kind !== "form") return true;
    const form = forms.find((f) => f.id === item.targetId);
    return !!form && (!!formTable(form) || form.controls.length > 0);
  });
}

/** The page trail's back link target, or null at the root of a trail. */
function usePageBack(runtime: RuntimeNavigation): BackLink | null {
  const { config } = useDocumentConfig();
  if (!runtime.canGoBack || !runtime.previous) return null;
  return { label: pageTitle(config, runtime.previous), onBack: runtime.back };
}

function NavList({ items, active }: { items: NavigationItem[]; active?: string }) {
  const { navigate } = useRuntimeNavigation();
  return (
    <ul>
      {items.map((item) => {
        const page = pageFor(item);
        return (
          <li key={item.id}>
            {item.kind === "group" ? (
              <details open>
                <summary>{item.label}</summary>
                <NavList items={item.children ?? []} active={active} />
              </details>
            ) : (
              <button
                type="button"
                aria-current={active === item.id ? "page" : undefined}
                onClick={() => page && navigate(page)}
              >
                {item.label}
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}

/** One runtime page; RBAC is re-checked here so direct navigation (actions) is enforced too. */
export function PageView({ page, back = null }: { page: RuntimePage; back?: BackLink | null }) {
  const { config } = useDocumentConfig();
  const { roleId } = useRuntimeNavigation();
  if (!canOpen(config, roleId, page))
    return (
      <p className="rt-error" role="alert">
        You do not have access to this page.
      </p>
    );
  switch (page.kind) {
    case "form":
      return (
        <FormRenderer
          formId={page.id}
          mode={page.mode as "list" | undefined}
          recordId={page.recordId}
          params={page.params}
          back={back}
        />
      );
    case "table":
      return <TablePage table={page.id} back={back} />;
    case "report":
      return (
        <>
          {back && <BackCrumb back={back} />}
          <ReportRun reportId={page.id} params={page.params} />
        </>
      );
    case "dashboard":
      return (
        <>
          {back && <BackCrumb back={back} />}
          <Embedded
            module={dashboardsModule}
            name="DashboardView"
            missing="Dashboards are not available in this build."
            dashboardId={page.id}
            params={page.params}
          />
        </>
      );
    default:
      return null;
  }
}

/** A table navigation item: the generated CRUD forms (with lookups and related lists), built in memory. */
function TablePage({ table, back }: { table: string; back: BackLink | null }) {
  const [context, setContext] = useState<Awaited<ReturnType<typeof tableContext>> | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    tableContext(table)
      .then(setContext)
      .catch((reason) => setError(reason instanceof Error ? reason.message : String(reason)));
  }, [table]);
  if (error)
    return (
      <p className="rt-error" role="alert">
        {error}
      </p>
    );
  if (!context) return <p className="rt-muted">Loading…</p>;
  const { schema, ...options } = context;
  return <FormRenderer formId={tableForms(schema, options).list.id} mode="list" back={back} />;
}
