import type { ComponentType } from "react";
import { AccessMigrationTab } from "../access";
import { readMigration } from "../access/migration";
import { CloudTab } from "../cloud";
import { FileSourcesTab } from "../import";
import { AssetsTab, LogsTab } from "../persistence";
import { ReleaseTab } from "../release";
import { RolesTab } from "../runtime";
import { MigrationsTab } from "../migrations";
import { DatasourceTab, EntitiesTab } from "../schema";
import { ProblemsTab } from "./ProblemsTab";
import type { DocumentConfig } from "../lib/types";
import { YamlTab } from "./YamlTab";

export type SettingsTabId =
  | "access"
  | "assets"
  | "release"
  | "cloud"
  | "datasource"
  | "entities"
  | "files"
  | "migrations"
  | "roles"
  | "yaml"
  | "problems"
  | "logs";

export interface SettingsTabDefinition {
  id: SettingsTabId;
  label: string;
  Component: ComponentType;
  // Shown only for documents it applies to (default: always).
  visible?: (config: DocumentConfig) => boolean;
}

/** Tabs of the `app` mode, in display order. Each Component lives in its owning feature dir. */
export const settingsTabs: readonly SettingsTabDefinition[] = [
  { id: "assets", label: "Assets", Component: AssetsTab },
  { id: "release", label: "Release", Component: ReleaseTab },
  { id: "cloud", label: "Cloud", Component: CloudTab },
  { id: "datasource", label: "Datasource", Component: DatasourceTab },
  { id: "entities", label: "Entities", Component: EntitiesTab },
  { id: "files", label: "File sources", Component: FileSourcesTab },
  { id: "migrations", label: "Migrations", Component: MigrationsTab },
  { id: "roles", label: "Roles", Component: RolesTab },
  { id: "yaml", label: "YAML", Component: YamlTab },
  { id: "problems", label: "Problems", Component: ProblemsTab },
  { id: "logs", label: "Logs", Component: LogsTab },
  {
    id: "access",
    label: "Access migration",
    Component: AccessMigrationTab,
    visible: (config) => readMigration(config.settings) !== null,
  },
];
