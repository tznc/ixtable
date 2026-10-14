//! Ordered conditional style rules of a control (port of `StylesEditor.tsx`): the first rule
//! whose condition holds applies.
//!
//! Test ids: the group `styles`, per rule n: `style-n-when` ("Style n when", with
//! `style-n-when-problem`), `style-n-tone`, `move-style-n-up`, `move-style-n-down`,
//! `remove-style-n`; and `add-style`.

use super::studio::DesignStudio;
use crate::design::{ConditionalStyle, Tone};
use crate::forms::schema::new_id;
use crate::ui::prelude::*;
use std::rc::Rc;

const TONES: [(&str, &str); 5] = [
    ("positive", "Positive"),
    ("negative", "Negative"),
    ("warning", "Warning"),
    ("muted", "Muted"),
    ("emphasis", "Emphasis"),
];

type Changed = Rc<dyn Fn(&mut DesignStudio, Vec<ConditionalStyle>, &mut Context<DesignStudio>)>;

impl DesignStudio {
    /// The rule list for `rules`; `names` are the scope names the conditions may use.
    pub(super) fn render_styles(
        &mut self,
        _owner: &str,
        rules: &[ConditionalStyle],
        names: Vec<String>,
        on_change: impl Fn(&mut Self, Vec<ConditionalStyle>, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let on_change: Changed = Rc::new(on_change);
        let muted = cx.theme().muted_foreground;
        let mut group = self.group("styles", "Conditional styles", cx);
        if rules.is_empty() {
            group = group.child(div().text_xs().text_color(muted).child("The first rule whose condition holds applies."));
        }
        let last = rules.len().saturating_sub(1);
        for (index, rule) in rules.iter().enumerate() {
            let n = index + 1;
            let list = rules.to_vec();
            let when = self.expression(
                &format!("style-{n}-when"),
                &format!("Style {n} when"),
                Some(rule.when.as_str()).filter(|w| !w.is_empty()),
                &[],
                Some(names.clone()),
                "value < 0",
                {
                    let (list, on_change) = (list.clone(), on_change.clone());
                    move |this, text, cx| {
                        let mut next = list.clone();
                        next[index].when = text.unwrap_or_default();
                        on_change(this, next, cx)
                    }
                },
                window,
                cx,
            );
            let tone = self.pick(
                &format!("style-{n}-tone"),
                &format!("Style {n} tone"),
                rule.tone.as_str(),
                super::studio::options(&TONES),
                "",
                {
                    let (list, on_change) = (list.clone(), on_change.clone());
                    move |this, value, cx| {
                        let mut next = list.clone();
                        next[index].tone = Tone::from(value.to_string());
                        on_change(this, next, cx)
                    }
                },
                cx,
            );
            let mover = |key: &str, icon: IconName, tip: &str, step: i64, disabled: bool, cx: &mut Context<Self>| {
                let (list, on_change) = (list.clone(), on_change.clone());
                super::ctl::icon_button(format!("{key}-{n}"), icon, tip).disabled(disabled).on_click(cx.listener(
                    move |this, _, _, cx| {
                        let mut next = list.clone();
                        let rule = next.remove(index);
                        next.insert((index as i64 + step) as usize, rule);
                        on_change(this, next, cx)
                    },
                ))
            };
            let up = mover("move-style-up", IconName::ArrowUp, &format!("Move style {n} up"), -1, index == 0, cx);
            let down = mover("move-style-down", IconName::ArrowDown, &format!("Move style {n} down"), 1, index == last, cx);
            let remove = {
                let (list, on_change) = (list.clone(), on_change.clone());
                super::ctl::icon_button(format!("remove-style-{n}"), IconName::Trash, &format!("Remove style {n}")).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let mut next = list.clone();
                        next.remove(index);
                        on_change(this, next, cx)
                    }),
                )
            };
            group = group.child(
                v_flex()
                    .id(SharedString::from(format!("style-{n}")))
                    .aria_label(format!("Style {n}"))
                    .gap_1()
                    .child(when)
                    .child(tone)
                    .child(h_flex().gap_1().child(up).child(down).child(remove))
                    .test_support(),
            );
        }
        let list = rules.to_vec();
        group.child(
            Button::new("add-style").outline().small().icon(IconName::Plus).label("Add style").on_click(cx.listener(
                move |this, _, _, cx| {
                    let mut next = list.clone();
                    next.push(ConditionalStyle { id: new_id(), when: String::new(), tone: Tone::Emphasis, column: None });
                    on_change(this, next, cx)
                },
            )),
        )
    }
}
