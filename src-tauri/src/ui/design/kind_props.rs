//! Kind-specific control properties (port of `KindProperties.tsx`): the pages of a tab group
//! and the child table, link columns and child form of a related-record list.
//!
//! Test ids: `tab-<n>-label` ("Tab n label"), `remove-tab-<n>`, `add-tab`; `related-table`,
//! `related-fk`, `related-parent`, `related-filter`, `related-form` ("Child form").

use super::studio::DesignStudio;
use crate::design::{Control, Form, RelatedList, TabPage};
use crate::forms::{conditions::filter_names, nesting::embeddable_forms, operations::remove_tab, schema::new_id};
use crate::ui::prelude::*;
use std::collections::HashSet;

impl DesignStudio {
    /// Tab pages of a tab group.
    pub(super) fn render_tabs_properties(
        &mut self,
        form: &Form,
        control: &Control,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let cid = control.id.clone();
        let mut group = self.group("tabs-properties", "Tabs", cx);
        for (index, tab) in control.tabs.iter().enumerate() {
            let n = index + 1;
            let tab_id = tab.id.clone();
            let label = self.text_field(
                &format!("tab-{n}-label"),
                &format!("Tab {n} label"),
                &tab.label,
                "",
                {
                    let (cid, tab_id) = (cid.clone(), tab_id.clone());
                    move |this, text, cx| {
                        let tab_id = tab_id.clone();
                        this.edit_control(
                            &cid,
                            "Edit control",
                            move |c| {
                                if let Some(t) = c.tabs.iter_mut().find(|t| t.id == tab_id) {
                                    t.label = text;
                                }
                            },
                            cx,
                        )
                    }
                },
                window,
                cx,
            );
            let remove = {
                let (cid, tab_id) = (cid.clone(), tab_id.clone());
                super::ctl::icon_button(format!("remove-tab-{n}"), IconName::Trash, &format!("Remove tab {}", tab.label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let (cid, tab_id) = (cid.clone(), tab_id.clone());
                        this.edit_form("Remove tab", move |f| *f = remove_tab(f, &cid, &tab_id), cx);
                    }))
            };
            group = group.child(h_flex().gap_2().items_end().child(label.flex_1()).child(remove));
        }
        let _ = form;
        let count = control.tabs.len();
        group.child(
            Button::new("add-tab")
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add tab")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let cid = cid.clone();
                    this.edit_control(
                        &cid,
                        "Edit control",
                        move |c| c.tabs.push(TabPage { id: new_id(), label: format!("Tab {}", count + 1) }),
                        cx,
                    );
                })),
        )
    }

    /// Child table, link columns, filter and child form of a related-record list.
    pub(super) fn render_related_properties(
        &mut self,
        form: &Form,
        control: &Control,
        columns: &[String],
        tables: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let cid = control.id.clone();
        let related = control.related.clone();
        let child_columns = self.columns_of(related.as_ref().map(|r| r.table.as_str()), cx);
        // Nested forms stay within three levels and never embed a form that embeds this one.
        let design_forms = self.design(cx).forms.clone();
        let allowed: HashSet<String> = embeddable_forms(&design_forms, form).iter().map(|f| f.id.clone()).collect();
        let current_form = related.as_ref().and_then(|r| r.form_id.clone());

        let mut table_options = vec![(String::new(), "—".to_string())];
        table_options.extend(tables.iter().map(|t| (t.clone(), t.clone())));
        let first_column = columns.first().cloned().unwrap_or_default();
        let table = self.pick(
            "related-table",
            "Child table",
            related.as_ref().map_or("", |r| r.table.as_str()),
            table_options,
            "—",
            {
                let cid = cid.clone();
                move |this, picked, cx| {
                    let (table, parent_column) = (picked.to_string(), first_column.clone());
                    this.edit_control(
                        &cid,
                        "Edit control",
                        move |c| {
                            c.related = Some(RelatedList {
                                table,
                                foreign_key: String::new(),
                                parent_column,
                                columns: vec![],
                                form_id: None,
                                ..Default::default()
                            })
                        },
                        cx,
                    )
                }
            },
            cx,
        );
        let fk = self.column_select(
            "related-fk",
            "Foreign key column",
            related.as_ref().map(|r| r.foreign_key.as_str()),
            &child_columns,
            related_edit(&cid, |r, v| r.foreign_key = v),
            cx,
        );
        let parent = self.column_select(
            "related-parent",
            "Parent column",
            related.as_ref().map(|r| r.parent_column.as_str()),
            columns,
            related_edit(&cid, |r, v| r.parent_column = v),
            cx,
        );
        let filter = self.expression(
            "related-filter",
            "Row filter",
            related.as_ref().and_then(|r| r.filter.as_deref()),
            &[],
            Some(filter_names(&child_columns)),
            "record.status <> 'void'",
            {
                let cid = cid.clone();
                move |this, v, cx| {
                    this.edit_control(&cid, "Edit control", move |c| {
                        if let Some(r) = c.related.as_mut() {
                            r.filter = v;
                        }
                    }, cx)
                }
            },
            window,
            cx,
        );
        let mut form_options = vec![(String::new(), "Generated from the table".to_string())];
        form_options.extend(
            design_forms
                .iter()
                .filter(|f| Some(&f.id) == current_form.as_ref() || allowed.contains(&f.id))
                .map(|f| (f.id.clone(), f.name.clone())),
        );
        let child_form = self.pick(
            "related-form",
            "Child form",
            current_form.as_deref().unwrap_or(""),
            form_options,
            "Generated from the table",
            related_edit(&cid, |r, v| r.form_id = (!v.is_empty()).then_some(v)),
            cx,
        );
        self.group("related-properties", "Related records", cx).child(table).child(fk).child(parent).child(filter).child(child_form)
    }
}

/// A choice handler that changes the related list of control `cid`.
fn related_edit(
    cid: &str,
    apply: impl Fn(&mut RelatedList, String) + Clone + 'static,
) -> impl Fn(&mut DesignStudio, &str, &mut Context<DesignStudio>) + 'static {
    let cid = cid.to_string();
    move |this, picked, cx| {
        let (value, apply) = (picked.to_string(), apply.clone());
        this.edit_control(
            &cid,
            "Edit control",
            move |c| {
                if let Some(r) = c.related.as_mut() {
                    apply(r, value);
                }
            },
            cx,
        )
    }
}
