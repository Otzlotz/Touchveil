use std::error::Error;
use std::ffi::{self, CStr};
use std::num::NonZeroU32;
use std::result::Result as StdResult;

use glutin::api::egl::context::PossiblyCurrentContext;
use glutin::api::egl::surface::Surface;
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::WindowSurface;
use slint::platform::femtovg_renderer::OpenGLInterface;

use crate::errors::{EGLError, Result};
use crate::logger;

pub struct EGLContext {
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
}

impl EGLContext {
    #[must_use]
    pub(super) fn from_raw(
        surface: Surface<WindowSurface>,
        context: PossiblyCurrentContext,
    ) -> Self {
        Self { surface, context }
    }

    fn ensure_current(&self) -> Result<()> {
        if !self.context.is_current() {
            self.context
                .make_current(&self.surface)
                .map_err(|e| EGLError::MakeCurrent { source: e.into() })?;
        }
        Ok(())
    }
}

impl Drop for EGLContext {
    fn drop(&mut self) {
        if self.context.is_current() {
            if let Err(e) = self.context.make_not_current_in_place() {
                logger::error!("Failed to make EGL context not current during cleanup: {e}");
            } else {
                logger::info!("Successfully made EGL context not current during cleanup");
            }
        }
    }
}

unsafe impl OpenGLInterface for EGLContext {
    fn ensure_current(&self) -> StdResult<(), Box<dyn Error + Send + Sync>> {
        self.ensure_current()
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }

    fn swap_buffers(&self) -> StdResult<(), Box<dyn Error + Send + Sync>> {
        self.surface
            .swap_buffers(&self.context)
            .map_err(|e| EGLError::SwapBuffers { source: e.into() }.into())
    }

    fn resize(
        &self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> StdResult<(), Box<dyn Error + Send + Sync>> {
        self.ensure_current()?;
        self.surface.resize(&self.context, width, height);
        Ok(())
    }

    fn get_proc_address(&self, name: &CStr) -> *const ffi::c_void {
        self.context.display().get_proc_address(name)
    }
}
