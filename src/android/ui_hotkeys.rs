use std::path::Path;

use super::App;

pub(super) struct State;

impl State {
    pub(super) fn new(_: &Path) -> Self { Self }
    pub(super) fn capturing(&self) -> bool { false }
}

pub(super) fn poll(_: &mut App, _: &egui::Context, _: &eframe::Frame) {}
pub(super) fn capture_dialog(_: &mut App, _: &egui::Context) {}
pub(super) fn settings_section(_: &mut App, _: &mut egui::Ui) {}
