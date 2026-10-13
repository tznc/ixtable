//! Form operations of the form designer: create, duplicate and delete. Every change keeps
//! pages in step (`pages::upgrade::sync`), so a new form gets a page and a deleted form's
//! page goes with it.

use super::studio::DesignStudio;
use crate::forms::{
    operations::{delete_form, duplicate_form},
    schema::new_form,
};
use crate::ui::prelude::*;

impl DesignStudio {
    pub fn create_form(&mut self, cx: &mut Context<Self>) -> String {
        let count = self.design(cx).forms.len();
        let form = new_form(&format!("Form {}", count + 1), None);
        let id = form.id.clone();
        self.select_form(&form.id, cx);
        self.edit("New form", move |c| {
            c.design.forms.push(form);
            crate::pages::upgrade::sync(c);
        }, cx);
        id
    }

    pub fn copy_form(&mut self, id: &str, cx: &mut Context<Self>) -> Option<String> {
        let source = self.design(cx).forms.iter().find(|f| f.id == id).cloned()?;
        let copy = duplicate_form(&source, None);
        let copy_id = copy.id.clone();
        self.select_form(&copy.id, cx);
        self.edit("Duplicate form", move |c| {
            c.design.forms.push(copy);
            crate::pages::upgrade::sync(c);
        }, cx);
        Some(copy_id)
    }

    pub fn remove_form(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.edit("Delete form", move |c| {
            c.design = delete_form(&c.design, &id);
            crate::pages::upgrade::sync(c);
        }, cx);
    }
}
