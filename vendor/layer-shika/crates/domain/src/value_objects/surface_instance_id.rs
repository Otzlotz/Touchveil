use crate::value_objects::handle::{Handle, Surface};
use crate::value_objects::output_handle::OutputHandle;

pub type SurfaceHandle = Handle<Surface>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceInstanceId {
    surface: SurfaceHandle,
    output: OutputHandle,
}

impl SurfaceInstanceId {
    pub const fn new(surface: SurfaceHandle, output: OutputHandle) -> Self {
        Self { surface, output }
    }

    pub const fn surface(self) -> SurfaceHandle {
        self.surface
    }

    pub const fn output(self) -> OutputHandle {
        self.output
    }
}
