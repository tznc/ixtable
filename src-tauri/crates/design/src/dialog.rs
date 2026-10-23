//! `Dialog`: one decision. The title is the question or the action, the
//! footer holds Cancel and exactly one primary (or a filled danger confirm).

use gpui_bootstrap::modal::{Modal, ModalSize};
use gpui_kit::base::DialogHandle;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, SharedString, Window, base::h_flex};

use crate::action::{ActionButton, Emphasis};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DialogSize {
    /// Confirmations.
    Small,
    /// Short forms.
    #[default]
    Medium,
    /// Pickers and previews.
    Large,
}

#[cfg_attr(feature = "styleguide", gpui_styleguidist::preview("dialog/confirm"))]
#[derive(IntoElement)]
pub struct Dialog {
    handle: DialogHandle,
    title: SharedString,
    size: DialogSize,
    cancel_label: SharedString,
    primary: Option<ActionButton>,
    children: Vec<AnyElement>,
}

impl Dialog {
    /// `title` asks or names the action: "Delete the Orders table?".
    pub fn new(handle: &DialogHandle, title: impl Into<SharedString>) -> Self {
        Self {
            handle: handle.clone(),
            title: title.into(),
            size: DialogSize::Medium,
            cancel_label: "Cancel".into(),
            primary: None,
            children: Vec::new(),
        }
    }

    pub fn size(mut self, size: DialogSize) -> Self {
        self.size = size;
        self
    }

    /// "Keep table" reads better than "Cancel" when the primary is destructive.
    pub fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// The one action that completes the dialog. A danger action becomes the
    /// filled confirm.
    pub fn primary(mut self, action: ActionButton) -> Self {
        self.primary = Some(match action.emphasis_of() {
            Emphasis::Danger => action.confirm(),
            _ => action.emphasis(Emphasis::Primary),
        });
        self
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Dialog {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let handle = self.handle.clone();
        let cancel = ActionButton::new("dialog-cancel", self.cancel_label)
            .on_click(move |_, window, cx| handle.close(window, cx));
        Modal::new(&self.handle)
            .title(self.title)
            .with_size(match self.size {
                DialogSize::Small => ModalSize::Small,
                DialogSize::Medium => ModalSize::Medium,
                DialogSize::Large => ModalSize::Large,
            })
            .children(self.children)
            .footer(h_flex().gap_2().child(cancel).children(self.primary))
    }
}
