import { useCallback } from "react";
import { inspectTable, listDatabaseObjects } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import type { TableSchema } from "../lib/types";
import { newId } from "../lib/utils";
import type { EntitySettings } from "../schema/types";
import { generateCrudForms, humanize } from "./generate";
import {
  type DesignForm,
  type DesignSchema,
  flattenNavigation,
  formTable,
  upgradeDesign,
} from "./schema";

/** A form that edits records of a table: the detail form a generated app links to. */
const isRecordForm = (form: DesignForm) => form.modes.includes("create");

/**
 * Adds generated list + detail forms and a navigation item for each table in `only`
 * (every table when omitted) that is missing them. A table counts as covered when any
 * form has it as its source, generated or built by hand; a table counts as navigable when
 * a navigation item opens one of its forms or the table itself. Running it again on its
 * own result changes nothing, so it never duplicates forms or navigation.
 */
export function addGeneratedApp(
  design: DesignSchema,
  schemas: TableSchema[],
  only?: string[],
  entities?: EntitySettings[],
): DesignSchema {
  const wanted = schemas.filter((schema) => !only || only.includes(schema.name));
  const covered = new Set(design.forms.map(formTable).filter(Boolean));
  const fresh = wanted.filter((schema) => !covered.has(schema.name));
  const targets = Object.fromEntries(schemas.map((schema) => [schema.name, schema]));
  const generated = fresh.map((schema) => ({
    schema,
    crud: generateCrudForms(schema, {
      targets,
      entities,
      children: schemas.filter((other) =>
        other.foreignKeys.some((fk) => fk.targetTable === schema.name),
      ),
    }),
  }));
  let forms = [...design.forms, ...generated.flatMap(({ crud }) => [crud.list, crud.detail])];
  // Embedded forms cannot nest related lists, so a nesting (or self) form gets a plain copy.
  const plain = new Map<string, DesignForm>();
  const childForm = (table: string, parent: DesignForm) => {
    const own = forms.find((form) => formTable(form) === table && isRecordForm(form));
    const nests = own?.controls.some((control) => control.kind === "relatedList");
    if (own && own.id !== parent.id && !nests) return own.id;
    const schema = targets[table];
    if (!schema) return null;
    if (!plain.has(table)) {
      const { detail } = generateCrudForms(schema, { targets, entities });
      plain.set(table, { ...detail, name: `${humanize(table)} (related)` });
    }
    return plain.get(table)?.id ?? null;
  };
  forms = forms.map((form) =>
    generated.some(({ crud }) => crud.detail === form)
      ? {
          ...form,
          controls: form.controls.map((control) =>
            control.related && !control.related.formId
              ? {
                  ...control,
                  related: {
                    ...control.related,
                    formId: childForm(control.related.table, form),
                  },
                }
              : control,
          ),
        }
      : form,
  );
  forms = [...forms, ...plain.values()];
  const navigable = new Set<string>();
  for (const item of flattenNavigation(design.navigation)) {
    if (item.kind === "table" && item.targetId) navigable.add(item.targetId);
    const table =
      item.kind === "form" ? formTable(forms.find((f) => f.id === item.targetId)) : null;
    if (table) navigable.add(table);
  }
  const navigation = [...design.navigation];
  for (const schema of wanted) {
    if (navigable.has(schema.name)) continue;
    const ours = generated.find((entry) => entry.schema === schema)?.crud.navigation;
    const list =
      forms.find((form) => formTable(form) === schema.name && form.modes.includes("list")) ??
      forms.find((form) => formTable(form) === schema.name);
    if (ours) navigation.push(ours);
    else if (list)
      navigation.push({
        id: newId(),
        label: list.name,
        kind: "form",
        targetId: list.id,
        children: [],
      });
  }
  if (forms.length === design.forms.length && navigation.length === design.navigation.length)
    return design;
  // A blank start page (none, or a form with no source and no controls) yields to the first new page.
  const start = flattenNavigation(navigation).find((item) => item.id === design.startPage);
  const startForm = start?.kind === "form" ? forms.find((f) => f.id === start.targetId) : null;
  const blank = !start || (startForm && !startForm.source && !startForm.controls.length);
  const firstNew = navigation[design.navigation.length]?.id;
  const startPage = blank && firstNew ? firstNew : (design.startPage ?? null);
  return { ...design, forms, navigation, startPage };
}

/** Schemas of every table in the open database (views are not generated). */
export async function tableSchemas(): Promise<TableSchema[]> {
  const objects = await listDatabaseObjects();
  const tables = objects.filter((object) => object.objectType === "table");
  return Promise.all(tables.map((table) => inspectTable(table.name)));
}

/**
 * "Generate app from tables" (and "Create forms for <table>"): one undoable config edit.
 * Resolves to the number of forms and navigation pages added (0 when nothing was missing).
 */
export function useGenerateApp() {
  const { config, update } = useDocumentConfig();
  return useCallback(
    async (only?: string[]) => {
      const schemas = await tableSchemas();
      // Nothing missing: no config write, so no empty undo step.
      const before = upgradeDesign(config.design);
      if (addGeneratedApp(before, schemas, only, config.entities) === before) return 0;
      let added = 0;
      await update(
        (draft) => {
          const design = upgradeDesign(draft.design);
          const next = addGeneratedApp(design, schemas, only, draft.entities);
          added =
            next.forms.length -
            design.forms.length +
            next.navigation.length -
            design.navigation.length;
          return next === design ? draft : { ...draft, design: next };
        },
        only?.length === 1 ? `Create forms for ${only[0]}` : "Generate app from tables",
      );
      return added;
    },
    [config.design, config.entities, update],
  );
}
