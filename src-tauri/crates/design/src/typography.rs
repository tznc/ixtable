//! The type scale. Six roles, no raw sizes: a screen has one `Title`, and
//! `Section` headings group what sits under it.

use gpui_kit::{FontWeight, Styled, px};

/// A role in the type scale. Sizes match the theme's headings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    /// 30px semibold. Welcome and empty-document screens only.
    Display,
    /// 20px semibold. The one heading per screen.
    Title,
    /// 16px semibold. Groups within a screen.
    Section,
    /// 14px. Everything people read.
    Body,
    /// 14px medium. Field labels, table headers, button text.
    Label,
    /// 12px. Hints, counts, timestamps.
    Caption,
}

impl Type {
    pub const ALL: [Type; 6] = [
        Type::Display,
        Type::Title,
        Type::Section,
        Type::Body,
        Type::Label,
        Type::Caption,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Type::Display => "Display",
            Type::Title => "Title",
            Type::Section => "Section",
            Type::Body => "Body",
            Type::Label => "Label",
            Type::Caption => "Caption",
        }
    }

    /// Font size in pixels.
    pub fn size(self) -> f32 {
        match self {
            Type::Display => 30.,
            Type::Title => 20.,
            Type::Section => 16.,
            Type::Body | Type::Label => 14.,
            Type::Caption => 12.,
        }
    }

    pub fn weight(self) -> FontWeight {
        match self {
            Type::Display | Type::Title | Type::Section => FontWeight::SEMIBOLD,
            Type::Label => FontWeight::MEDIUM,
            Type::Body | Type::Caption => FontWeight::NORMAL,
        }
    }
}

/// `.ty(Type::Title)` on any styled element.
pub trait TypeScale: Styled + Sized {
    fn ty(self, ty: Type) -> Self {
        self.text_size(px(ty.size())).font_weight(ty.weight())
    }
}

impl<E: Styled> TypeScale for E {}

/// What a message means. One tone per meaning, so color is never decoration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    /// Neutral facts: "Draft", "3 rows".
    #[default]
    Neutral,
    /// Something to know before acting.
    Info,
    /// It worked.
    Success,
    /// It worked, but look at this.
    Warning,
    /// It did not work, or it cannot be undone.
    Danger,
}

impl Tone {
    pub const ALL: [Tone; 5] = [
        Tone::Neutral,
        Tone::Info,
        Tone::Success,
        Tone::Warning,
        Tone::Danger,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tone::Neutral => "neutral",
            Tone::Info => "info",
            Tone::Success => "success",
            Tone::Warning => "warning",
            Tone::Danger => "danger",
        }
    }

    /// The Bootstrap variant a tone renders with. Info uses the brand teal
    /// rather than Bootstrap's cyan, so the app has one cool accent.
    pub fn variant(self) -> gpui_bootstrap::Variant {
        use gpui_bootstrap::Variant;
        match self {
            Tone::Neutral => Variant::Secondary,
            Tone::Info => Variant::Primary,
            Tone::Success => Variant::Success,
            Tone::Warning => Variant::Warning,
            Tone::Danger => Variant::Danger,
        }
    }
}
