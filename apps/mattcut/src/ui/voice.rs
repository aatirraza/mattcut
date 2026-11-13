//! Isolate voice (Keep vocal / Remove vocal) in the clip menu. The separation itself is the next
//! step; until it lands, this adds nothing to the menu.

use super::MattCut;
use crate::model::{ClipView, Kind};

pub fn menu(_app: &mut MattCut, _ui: &mut egui::Ui, _clip: &ClipView, _kind: Kind, _partner: Option<&ClipView>) {}
