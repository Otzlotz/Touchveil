use std::cell::RefCell;
use std::rc::Rc;

use slint::PlatformError;
use slint::platform::{Platform, WindowAdapter};

use crate::rendering::femtovg::main_window::FemtoVGWindow;

pub struct CustomSlintPlatform {
    pending_windows: RefCell<Vec<Rc<FemtoVGWindow>>>,
}

impl CustomSlintPlatform {
    #[must_use]
    pub fn new(window: &Rc<FemtoVGWindow>) -> Rc<Self> {
        Rc::new(Self {
            pending_windows: RefCell::new(vec![Rc::clone(window)]),
        })
    }

    #[must_use]
    pub fn new_empty() -> Rc<Self> {
        Rc::new(Self {
            pending_windows: RefCell::new(Vec::new()),
        })
    }

    pub fn add_window(&self, window: Rc<FemtoVGWindow>) {
        self.pending_windows.borrow_mut().push(window);
    }
}

impl Platform for CustomSlintPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter + 'static>, PlatformError> {
        let mut windows = self.pending_windows.borrow_mut();
        if windows.is_empty() {
            Err(PlatformError::NoPlatform)
        } else {
            let window = windows.remove(0);
            Ok(window as Rc<dyn WindowAdapter>)
        }
    }
}
