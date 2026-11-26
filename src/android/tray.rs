#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Show, Toggle, PlayPause, Next, Prev, Quit,
}

pub struct Tray;

impl Tray {
    pub fn new(_: &crate::appicon::Variant, _: impl Fn() + Send + Sync + Clone + 'static) -> anyhow::Result<Self> {
        anyhow::bail!("The Android app uses media notifications")
    }
    pub fn set_shown(&mut self, _: bool) {}
    pub fn set_now(&mut self, _: Option<(&str, &str)>, _: bool) {}
}

pub fn take() -> Vec<Cmd> { Vec::new() }
