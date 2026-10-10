import { type KeyboardEvent, useEffect, useState } from "react";
import { appInfo } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import { type SettingsTabId, settingsTabs as allTabs } from "./settings-tabs";

/** The `app` mode: application settings as a tab strip over `settingsTabs`. */
export function AppSettings() {
  const [active, setActive] = useState<SettingsTabId>("assets");
  const [runtime, setRuntime] = useState("");
  const { config } = useDocumentConfig();
  const settingsTabs = allTabs.filter((tab) => !tab.visible || tab.visible(config));
  useEffect(() => {
    appInfo()
      .then((info) => setRuntime(`${info.name} · ${info.runtime} runtime`))
      .catch(() => setRuntime(""));
  }, []);
  const tab = settingsTabs.find((item) => item.id === active) ?? settingsTabs[0];
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = settingsTabs.findIndex((item) => item.id === active);
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const next = settingsTabs[(index + step + settingsTabs.length) % settingsTabs.length];
    setActive(next.id);
    document.getElementById(`settings-tab-${next.id}`)?.focus();
  };
  return (
    <section className="app-settings" aria-label="Application settings">
      <header className="titlebar">
        <div>
          <p>PROJECT / SETTINGS</p>
          <h1>Application settings</h1>
        </div>
        {runtime && <small className="runtime-info">{runtime}</small>}
      </header>
      <div className="settings-tabs" role="tablist" aria-label="Settings" onKeyDown={onKeyDown}>
        {settingsTabs.map((item) => (
          <button
            key={item.id}
            id={`settings-tab-${item.id}`}
            role="tab"
            aria-selected={item.id === active}
            aria-controls="settings-panel"
            tabIndex={item.id === active ? 0 : -1}
            className={item.id === active ? "active" : ""}
            onClick={() => setActive(item.id)}
          >
            {item.label}
          </button>
        ))}
      </div>
      <div
        id="settings-panel"
        role="tabpanel"
        aria-labelledby={`settings-tab-${tab.id}`}
        className="settings-body"
      >
        <tab.Component />
      </div>
    </section>
  );
}
