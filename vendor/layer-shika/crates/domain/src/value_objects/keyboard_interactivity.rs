/// Controls how a surface receives keyboard input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyboardInteractivity {
    /// Surface does not receive keyboard events
    None,
    /// Surface always receives keyboard focus
    Exclusive,
    /// Surface receives focus when clicked (default)
    #[default]
    OnDemand,
}
