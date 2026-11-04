//! The three package tasks and how their results are stacked as new layers.

use photocraft_color::BlendMode;

use crate::scan::Catalog;

/// Apply order when Filter › Portrait retouch › All runs: heal underneath, skin tone on top.
pub const STACK_ORDER: [Task; 3] = [Task::Heal, Task::DodgeBurn, Task::SkinTone];

/// Soft Light opacity for Dodge & Burn, alone and in the All stack.
pub const DODGE_BURN_OPACITY: f32 = 0.85;

/// A package task. The directory name from `retouch package` / `dist/<task>` is the id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Task {
    Heal,
    DodgeBurn,
    SkinTone,
}

/// Which Filter › Portrait retouch item a caller is asking about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuWhich {
    Task(Task),
    All,
}

/// Blend mode, opacity and layer name for one new layer. Not a destructive paint.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerPlacement {
    pub blend: BlendMode,
    pub opacity: f32,
    pub name: String,
}

impl Task {
    pub const fn id(self) -> &'static str {
        match self {
            Self::Heal => "heal",
            Self::DodgeBurn => "dodge_burn",
            Self::SkinTone => "skin_tone",
        }
    }

    pub const fn menu_label(self) -> &'static str {
        match self {
            Self::Heal => "Heal",
            Self::DodgeBurn => "Dodge & Burn",
            Self::SkinTone => "Skin Tone",
        }
    }

    pub const fn command_id(self) -> &'static str {
        match self {
            Self::Heal => "filter.portraitRetouch.heal",
            Self::DodgeBurn => "filter.portraitRetouch.dodgeBurn",
            Self::SkinTone => "filter.portraitRetouch.skinTone",
        }
    }

    /// `heal`, `dodge_burn`, `dodge-burn`, `skin_tone`, `skin-tone`, and the same without underscores.
    pub fn parse(raw: &str) -> Option<Self> {
        let key: String = raw.trim().to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        match key.as_str() {
            "heal" => Some(Self::Heal),
            "dodgeburn" | "dodgeandburn" => Some(Self::DodgeBurn),
            "skintone" => Some(Self::SkinTone),
            _ => None,
        }
    }

    fn default_placement(self, stacked: bool) -> LayerPlacement {
        let (blend, opacity) = match self {
            Self::Heal => (BlendMode::Normal, 1.0),
            Self::DodgeBurn => (BlendMode::SoftLight, DODGE_BURN_OPACITY),
            // Color only in a multi-filter stack, so a lone Skin Tone layer keeps the model's RGB.
            Self::SkinTone if stacked => (BlendMode::Color, 1.0),
            Self::SkinTone => (BlendMode::Normal, 1.0),
        };
        LayerPlacement { blend, opacity, name: self.menu_label().to_string() }
    }
}

/// Resolved placement for `task`. `stacked` is true when All is placing more than one layer.
pub fn placement(solo: &LayerPlacement, stacked_spec: &LayerPlacement, stacked: bool) -> LayerPlacement {
    if stacked { stacked_spec.clone() } else { solo.clone() }
}

impl Task {
    pub(crate) fn defaults(self) -> (LayerPlacement, LayerPlacement) {
        (self.default_placement(false), self.default_placement(true))
    }
}

/// Why a menu item is disabled, or `None` when it can run.
///
/// A missing package is reported before a missing ONNX Runtime, so an empty install says where
/// the package is missing rather than talking about the build.
pub fn disable_reason(catalog: &Catalog, runtime: bool, which: MenuWhich) -> Option<String> {
    let missing = match which {
        MenuWhich::Task(task) => {
            if catalog.get(task).is_none() {
                vec![task.id()]
            } else {
                Vec::new()
            }
        }
        MenuWhich::All => STACK_ORDER.into_iter().filter(|task| catalog.get(*task).is_none()).map(Task::id).collect(),
    };
    if !missing.is_empty() {
        let dir = catalog.directory.display();
        return if matches!(which, MenuWhich::Task(_)) {
            Some(format!("portrait filter package `{}` is not installed ({dir})", missing[0]))
        } else {
            Some(format!("missing portrait filter package: {} ({dir})", missing.join(", ")))
        };
    }
    if !runtime {
        return Some("this build does not include ONNX Runtime; rebuild with --features portrait-onnx".to_string());
    }
    None
}
