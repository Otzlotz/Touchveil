use layer_shika_adapters::platform::slint_interpreter::ComponentInstance;

/// Default surface name used internally
pub const DEFAULT_SURFACE_NAME: &str = "main";

/// Trait providing runtime access to shell components and event loop
pub trait ShellRuntime {
    type LoopHandle;
    type Context<'a>;

    fn event_loop_handle(&self) -> Self::LoopHandle;

    fn with_component<F>(&self, name: &str, f: F)
    where
        F: FnMut(&ComponentInstance);

    fn with_all_components<F>(&self, f: F)
    where
        F: FnMut(&str, &ComponentInstance);

    fn run(&mut self) -> crate::Result<()>;
}
