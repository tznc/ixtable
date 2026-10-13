//! A small drop-down used by the query views: a button showing the current option that
//! opens a list of options. The owner keeps the open state, so many of these can sit in
//! one view without an entity each.
//!
//! Test ids: the trigger has the id given; each option is `<id>/<value>`.

use crate::ui::prelude::*;
use gpui_kit::component::ThemeStyled as _;
use std::rc::Rc;

/// One selectable option.
#[derive(Clone, Debug, PartialEq)]
pub struct Opt {
    pub value: String,
    pub label: String,
}

impl Opt {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
    /// Value and label are the same text.
    pub fn same(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            value: text.clone(),
            label: text,
        }
    }
}

pub struct Choice<V: 'static> {
    pub id: SharedString,
    pub current: String,
    pub options: Vec<Opt>,
    pub open: bool,
    pub disabled: bool,
    /// Text of the trigger instead of the current option's label.
    pub trigger_label: Option<String>,
    pub set_open: Rc<dyn Fn(&mut V, bool, &mut Context<V>)>,
    pub on_pick: Rc<dyn Fn(&mut V, String, &mut Window, &mut Context<V>)>,
}

impl<V: 'static> Choice<V> {
    pub fn new(
        id: impl Into<SharedString>,
        current: impl Into<String>,
        options: Vec<Opt>,
        open: bool,
        set_open: impl Fn(&mut V, bool, &mut Context<V>) + 'static,
        on_pick: impl Fn(&mut V, String, &mut Window, &mut Context<V>) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            current: current.into(),
            options,
            open,
            disabled: false,
            trigger_label: None,
            set_open: Rc::new(set_open),
            on_pick: Rc::new(on_pick),
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.trigger_label = Some(label.into());
        self
    }

    /// The label of the current option (or the raw value when it is not listed).
    pub fn current_label(&self) -> String {
        self.options
            .iter()
            .find(|o| o.value == self.current)
            .map_or_else(|| self.current.clone(), |o| o.label.clone())
    }

    pub fn render(self, cx: &mut Context<V>) -> AnyElement {
        let label = self.trigger_label.clone().unwrap_or_else(|| self.current_label());
        let open = self.open;
        let id = self.id.clone();
        let toggle = {
            let set_open = self.set_open.clone();
            cx.listener(move |this, _, _, cx| set_open(this, !open, cx))
        };
        let trigger = Button::new(id.clone())
            .outline()
            .small()
            .label(if label.is_empty() { "—".to_string() } else { label })
            .disabled(self.disabled)
            .on_click(toggle);
        let list = self.open.then(|| {
            let close = {
                let set_open = self.set_open.clone();
                cx.listener(move |this, _, _, cx| set_open(this, false, cx))
            };
            let items = self.options.iter().map(|opt| {
                let on_pick = self.on_pick.clone();
                let set_open = self.set_open.clone();
                let value = opt.value.clone();
                Button::new(SharedString::from(format!("{id}/{}", opt.value)))
                    .ghost()
                    .small()
                    .justify_start()
                    .w_full()
                    .label(opt.label.clone())
                    .selected(opt.value == self.current)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        set_open(this, false, cx);
                        on_pick(this, value.clone(), window, cx);
                    }))
            });
            deferred(
                v_flex()
                    .id(SharedString::from(format!("{id}-options")))
                    .absolute()
                    .top(px(30.))
                    .left_0()
                    .min_w(px(180.))
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .occlude()
                    .popover_style(cx)
                    .p_1()
                    .children(items)
                    .on_mouse_down_out(close),
            )
            .with_priority(2)
        });
        div().relative().child(trigger).children(list).into_any_element()
    }
}
