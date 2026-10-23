use crate::value_objects::output_handle::OutputHandle;

/// Runtime information about a connected output (monitor)
#[derive(Debug, Clone, PartialEq)]
pub struct OutputInfo {
    handle: OutputHandle,
    name: Option<String>,
    description: Option<String>,
    geometry: Option<OutputGeometry>,
    scale: Option<i32>,
    is_primary: bool,
}

/// Physical geometry and properties of an output
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputGeometry {
    pub x: i32,
    pub y: i32,
    pub physical_width: i32,
    pub physical_height: i32,
    pub make: Option<String>,
    pub model: Option<String>,
}

impl OutputInfo {
    pub fn new(handle: OutputHandle) -> Self {
        Self {
            handle,
            name: None,
            description: None,
            geometry: None,
            scale: None,
            is_primary: false,
        }
    }

    pub const fn handle(&self) -> OutputHandle {
        self.handle
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub const fn geometry(&self) -> Option<&OutputGeometry> {
        self.geometry.as_ref()
    }

    pub const fn scale(&self) -> Option<i32> {
        self.scale
    }

    pub const fn is_primary(&self) -> bool {
        self.is_primary
    }

    pub fn set_name(&mut self, name: String) {
        self.name = Some(name);
    }

    pub fn set_description(&mut self, description: String) {
        self.description = Some(description);
    }

    pub fn set_geometry(&mut self, geometry: OutputGeometry) {
        self.geometry = Some(geometry);
    }

    pub fn set_scale(&mut self, scale: i32) {
        self.scale = Some(scale);
    }

    pub fn set_primary(&mut self, is_primary: bool) {
        self.is_primary = is_primary;
    }
}

impl OutputGeometry {
    pub const fn new(x: i32, y: i32, physical_width: i32, physical_height: i32) -> Self {
        Self {
            x,
            y,
            physical_width,
            physical_height,
            make: None,
            model: None,
        }
    }

    #[must_use]
    pub fn with_make(mut self, make: String) -> Self {
        self.make = Some(make);
        self
    }

    #[must_use]
    pub fn with_model(mut self, model: String) -> Self {
        self.model = Some(model);
        self
    }
}
