use std::cell::RefCell;
use std::collections::HashMap;
use std::os::fd::BorrowedFd;
use std::rc::Rc;

use layer_shika_domain::entities::output_registry::OutputRegistry;
use layer_shika_domain::value_objects::handle::SurfaceHandle;
use layer_shika_domain::value_objects::output_handle::OutputHandle;
use layer_shika_domain::value_objects::output_info::OutputInfo;
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_keyboard;
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Proxy, QueueHandle};
use xkbcommon::xkb;

use super::keyboard_state::KeyboardState;
use super::surface_state::SurfaceState;
use crate::errors::{RenderingError, Result};
use crate::rendering::femtovg::renderable_window::RenderableWindow;
use crate::wayland::globals::context::GlobalContext;
use crate::wayland::input::KeyboardInputState;
use crate::wayland::managed_proxies::{ManagedWlKeyboard, ManagedWlPointer};
use crate::wayland::outputs::{OutputManager, OutputMapping};
use crate::wayland::rendering::RenderableSet;

pub type PerOutputSurface = SurfaceState;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShellSurfaceKey {
    pub output_handle: OutputHandle,
    pub surface_handle: SurfaceHandle,
}

impl ShellSurfaceKey {
    pub const fn new(output_handle: OutputHandle, surface_handle: SurfaceHandle) -> Self {
        Self {
            output_handle,
            surface_handle,
        }
    }
}

pub struct AppState {
    global_context: Option<Rc<GlobalContext>>,
    output_registry: OutputRegistry,
    output_mapping: OutputMapping,
    surfaces: HashMap<ShellSurfaceKey, PerOutputSurface>,
    surface_to_key: HashMap<ObjectId, ShellSurfaceKey>,
    surface_handle_to_name: HashMap<SurfaceHandle, String>,
    /// Only present while the seat reports the capability.
    pointer: Option<ManagedWlPointer>,
    keyboard: Option<ManagedWlKeyboard>,
    output_manager: Option<Rc<RefCell<OutputManager>>>,
    registry_name_to_output_id: HashMap<u32, ObjectId>,
    active_surface_key: Option<ShellSurfaceKey>,
    keyboard_focus_key: Option<ShellSurfaceKey>,
    keyboard_input_state: KeyboardInputState,
    keyboard_state: KeyboardState,
    queue_handle: Option<wayland_client::QueueHandle<AppState>>,
    /// Latest current mode per output. Mode events can arrive before the surfaces
    /// are created (output prefetch); the size is then applied later.
    output_modes: HashMap<ObjectId, (i32, i32)>,
}

impl AppState {
    pub(crate) fn new() -> Self {
        Self {
            global_context: None,
            output_registry: OutputRegistry::new(),
            output_mapping: OutputMapping::new(),
            surfaces: HashMap::new(),
            surface_to_key: HashMap::new(),
            surface_handle_to_name: HashMap::new(),
            pointer: None,
            keyboard: None,
            output_manager: None,
            registry_name_to_output_id: HashMap::new(),
            active_surface_key: None,
            keyboard_focus_key: None,
            keyboard_input_state: KeyboardInputState::new(),
            keyboard_state: KeyboardState::new(),
            queue_handle: None,
            output_modes: HashMap::new(),
        }
    }

    pub fn set_global_context(&mut self, context: Rc<GlobalContext>) {
        self.global_context = Some(context);
    }

    pub fn set_queue_handle(&mut self, queue_handle: wayland_client::QueueHandle<AppState>) {
        self.queue_handle = Some(queue_handle);
    }

    pub fn set_output_manager(&mut self, manager: Rc<RefCell<OutputManager>>) {
        self.output_manager = Some(manager);
    }

    pub fn output_manager(&self) -> Option<Rc<RefCell<OutputManager>>> {
        self.output_manager.as_ref().map(Rc::clone)
    }

    pub fn register_registry_name(&mut self, name: u32, output_id: ObjectId) {
        self.registry_name_to_output_id.insert(name, output_id);
    }

    pub fn find_output_id_by_registry_name(&self, name: u32) -> Option<ObjectId> {
        self.registry_name_to_output_id.get(&name).cloned()
    }

    pub fn unregister_registry_name(&mut self, name: u32) -> Option<ObjectId> {
        self.registry_name_to_output_id.remove(&name)
    }

    pub fn add_shell_surface(
        &mut self,
        output_id: &ObjectId,
        surface_handle: SurfaceHandle,
        surface_name: &str,
        main_surface_id: ObjectId,
        mut surface_state: PerOutputSurface,
    ) {
        if let Some(&(width, height)) = self.output_modes.get(output_id) {
            surface_state.handle_output_mode(width, height);
        }
        let handle = self.output_mapping.get(output_id).unwrap_or_else(|| {
            let h = self.output_mapping.insert(output_id.clone());
            let is_primary = self.output_registry.is_empty();
            let mut info = OutputInfo::new(h);
            info.set_primary(is_primary);
            self.output_registry.add(info);
            h
        });

        let key = ShellSurfaceKey::new(handle, surface_handle);
        self.surface_to_key.insert(main_surface_id, key.clone());
        self.surfaces.insert(key, surface_state);
        self.surface_handle_to_name
            .insert(surface_handle, surface_name.to_string());
    }

    pub fn add_output(
        &mut self,
        output_id: &ObjectId,
        surface_handle: SurfaceHandle,
        surface_name: &str,
        main_surface_id: ObjectId,
        surface_state: PerOutputSurface,
    ) {
        self.add_shell_surface(
            output_id,
            surface_handle,
            surface_name,
            main_surface_id,
            surface_state,
        );
    }

    pub fn remove_output(&mut self, handle: OutputHandle) -> Vec<PerOutputSurface> {
        self.output_registry.remove(handle);

        let keys_to_remove: Vec<_> = self
            .surfaces
            .keys()
            .filter(|k| k.output_handle == handle)
            .cloned()
            .collect();

        let mut removed = Vec::new();
        for key in keys_to_remove {
            if let Some(surface) = self.surfaces.remove(&key) {
                removed.push(surface);
            }
        }

        self.surface_to_key.retain(|_, k| k.output_handle != handle);

        removed
    }

    pub fn get_surface_by_key(&self, key: &ShellSurfaceKey) -> Option<&PerOutputSurface> {
        self.surfaces.get(key)
    }

    pub fn get_surface_by_key_mut(
        &mut self,
        key: &ShellSurfaceKey,
    ) -> Option<&mut PerOutputSurface> {
        self.surfaces.get_mut(key)
    }

    pub fn get_surface_by_instance(
        &self,
        surface_handle: SurfaceHandle,
        output_handle: OutputHandle,
    ) -> Option<&PerOutputSurface> {
        let key = ShellSurfaceKey::new(output_handle, surface_handle);
        self.surfaces.get(&key)
    }

    pub fn get_surface_by_instance_mut(
        &mut self,
        surface_handle: SurfaceHandle,
        output_handle: OutputHandle,
    ) -> Option<&mut PerOutputSurface> {
        let key = ShellSurfaceKey::new(output_handle, surface_handle);
        self.surfaces.get_mut(&key)
    }

    pub fn get_output_by_output_id(&self, output_id: &ObjectId) -> Option<&PerOutputSurface> {
        self.output_mapping
            .get(output_id)
            .and_then(|handle| self.get_first_surface_for_output(handle))
    }

    pub fn get_output_by_output_id_mut(
        &mut self,
        output_id: &ObjectId,
    ) -> Option<&mut PerOutputSurface> {
        self.output_mapping
            .get(output_id)
            .and_then(|handle| self.get_first_surface_for_output_mut(handle))
    }

    fn get_first_surface_for_output(&self, handle: OutputHandle) -> Option<&PerOutputSurface> {
        self.surfaces
            .iter()
            .find(|(k, _)| k.output_handle == handle)
            .map(|(_, v)| v)
    }

    fn get_first_surface_for_output_mut(
        &mut self,
        handle: OutputHandle,
    ) -> Option<&mut PerOutputSurface> {
        self.surfaces
            .iter_mut()
            .find(|(k, _)| k.output_handle == handle)
            .map(|(_, v)| v)
    }

    pub fn get_output_by_surface(&self, surface_id: &ObjectId) -> Option<&PerOutputSurface> {
        self.surface_to_key
            .get(surface_id)
            .and_then(|key| self.surfaces.get(key))
    }

    pub fn get_output_by_surface_mut(
        &mut self,
        surface_id: &ObjectId,
    ) -> Option<&mut PerOutputSurface> {
        self.surface_to_key
            .get(surface_id)
            .and_then(|key| self.surfaces.get_mut(key))
    }

    pub fn get_output_by_layer_surface_mut(
        &mut self,
        layer_surface_id: &ObjectId,
    ) -> Option<&mut PerOutputSurface> {
        self.surfaces
            .values_mut()
            .find(|surface| surface.layer_surface().as_ref().id() == *layer_surface_id)
    }

    pub fn get_surface_name(&self, surface_handle: SurfaceHandle) -> Option<&str> {
        self.surface_handle_to_name
            .get(&surface_handle)
            .map(String::as_str)
    }

    pub fn get_key_by_surface(&self, surface_id: &ObjectId) -> Option<&ShellSurfaceKey> {
        self.surface_to_key.get(surface_id)
    }

    pub fn get_handle_by_surface(&self, surface_id: &ObjectId) -> Option<OutputHandle> {
        self.surface_to_key
            .get(surface_id)
            .map(|key| key.output_handle)
    }

    pub fn set_output_mode(&mut self, output_id: &ObjectId, width: i32, height: i32) {
        self.output_modes.insert(output_id.clone(), (width, height));
    }

    pub fn get_handle_by_output_id(&self, output_id: &ObjectId) -> Option<OutputHandle> {
        self.output_mapping.get(output_id)
    }

    pub fn ensure_output_registered(&mut self, output_id: &ObjectId) -> OutputHandle {
        self.output_mapping.get(output_id).unwrap_or_else(|| {
            let h = self.output_mapping.insert(output_id.clone());
            let is_primary = self.output_registry.is_empty();
            let mut info = OutputInfo::new(h);
            info.set_primary(is_primary);
            self.output_registry.add(info);
            h
        })
    }

    pub fn set_active_output_handle(&mut self, handle: Option<OutputHandle>) {
        self.output_registry.set_active(handle);
    }

    pub fn active_output_handle(&self) -> Option<OutputHandle> {
        self.output_registry.active_handle()
    }

    pub fn set_active_surface_key(&mut self, key: Option<ShellSurfaceKey>) {
        if let Some(ref k) = key {
            self.output_registry.set_active(Some(k.output_handle));
        } else {
            self.output_registry.set_active(None);
        }
        self.active_surface_key = key;
    }

    pub fn active_surface_key(&self) -> Option<&ShellSurfaceKey> {
        self.active_surface_key.as_ref()
    }

    pub fn active_surface_mut(&mut self) -> Option<&mut PerOutputSurface> {
        let key = self.active_surface_key.clone()?;
        self.surfaces.get_mut(&key)
    }

    pub fn primary_output(&self) -> Option<&PerOutputSurface> {
        self.output_registry
            .primary_handle()
            .and_then(|handle| self.get_first_surface_for_output(handle))
    }

    pub fn primary_output_handle(&self) -> Option<OutputHandle> {
        self.output_registry.primary_handle()
    }

    pub fn active_output(&self) -> Option<&PerOutputSurface> {
        self.output_registry
            .active_handle()
            .and_then(|handle| self.get_first_surface_for_output(handle))
    }

    pub fn all_outputs(&self) -> impl Iterator<Item = &PerOutputSurface> {
        self.surfaces.values()
    }

    pub fn all_outputs_mut(&mut self) -> impl Iterator<Item = &mut PerOutputSurface> {
        self.surfaces.values_mut()
    }

    pub fn surfaces_for_output(
        &self,
        handle: OutputHandle,
    ) -> impl Iterator<Item = (&str, &PerOutputSurface)> + '_ {
        self.surfaces
            .iter()
            .filter(move |(k, _)| k.output_handle == handle)
            .map(|(k, v)| {
                let name = self
                    .surface_handle_to_name
                    .get(&k.surface_handle)
                    .map_or("unknown", String::as_str);
                (name, v)
            })
    }

    pub fn surfaces_with_keys(
        &self,
    ) -> impl Iterator<Item = (&ShellSurfaceKey, &PerOutputSurface)> {
        self.surfaces.iter()
    }

    /// Create or release pointer and keyboard to match the seat's capabilities.
    /// Without a mouse or keyboard, `get_pointer`/`get_keyboard` must not be called;
    /// wlroots compositors otherwise close the connection with a protocol error.
    pub fn set_seat_capabilities(
        &mut self,
        seat: &WlSeat,
        caps: wl_seat::Capability,
        qh: &QueueHandle<Self>,
        connection: &Connection,
    ) {
        let has_pointer = caps.contains(wl_seat::Capability::Pointer);
        if has_pointer && self.pointer.is_none() {
            let pointer = Rc::new(seat.get_pointer(qh, ()));
            self.pointer = Some(ManagedWlPointer::new(pointer, Rc::new(connection.clone())));
        } else if !has_pointer {
            self.pointer = None;
        }

        let has_keyboard = caps.contains(wl_seat::Capability::Keyboard);
        if has_keyboard && self.keyboard.is_none() {
            let keyboard = Rc::new(seat.get_keyboard(qh, ()));
            self.keyboard = Some(ManagedWlKeyboard::new(
                keyboard,
                Rc::new(connection.clone()),
            ));
        } else if !has_keyboard {
            self.keyboard = None;
        }
    }

    pub fn handle_keymap(&mut self, fd: BorrowedFd<'_>, size: u32) {
        let Ok(fd) = fd.try_clone_to_owned() else {
            return;
        };

        let keymap = unsafe {
            xkb::Keymap::new_from_fd(
                &self.keyboard_state.xkb_context,
                fd,
                size as usize,
                xkb::KEYMAP_FORMAT_TEXT_V1,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
        };

        if let Ok(Some(keymap)) = keymap {
            self.keyboard_state.set_keymap(keymap);
        }
    }

    pub fn handle_keyboard_enter(&mut self, _serial: u32, surface: &WlSurface, _keys: &[u8]) {
        let surface_id = surface.id();
        if let Some(key) = self.get_key_by_surface(&surface_id).cloned() {
            self.keyboard_input_state
                .set_focused_surface(Some(surface_id));
            self.set_keyboard_focus(Some(key));
        }
    }

    pub fn handle_keyboard_leave(&mut self, _serial: u32, surface: &WlSurface) {
        let surface_id = surface.id();
        if self.keyboard_input_state.focused_surface_id() == Some(&surface_id) {
            self.keyboard_input_state.reset();
            self.set_keyboard_focus(None);
        }
    }

    pub fn handle_key(&mut self, _serial: u32, _time: u32, key: u32, state: wl_keyboard::KeyState) {
        let Some(focus_key) = self.keyboard_focus_key.clone() else {
            return;
        };
        let Some(surface_id) = self.keyboard_input_state.focused_surface_id().cloned() else {
            return;
        };

        if let Some(surface) = self.surfaces.get_mut(&focus_key) {
            surface.handle_keyboard_key(&surface_id, key, state, &mut self.keyboard_state);
        }
    }

    pub fn handle_modifiers(
        &mut self,
        _serial: u32,
        mods_depressed: u32,
        mods_latched: u32,
        mods_locked: u32,
        group: u32,
    ) {
        if let Some(state) = self.keyboard_state.xkb_state.as_mut() {
            state.update_mask(mods_depressed, mods_latched, mods_locked, 0, 0, group);
        }
    }

    pub fn handle_repeat_info(&mut self, rate: i32, delay: i32) {
        self.keyboard_state.repeat_rate = rate;
        self.keyboard_state.repeat_delay = delay;
    }

    fn set_keyboard_focus(&mut self, key: Option<ShellSurfaceKey>) {
        if let Some(ref k) = key {
            self.output_registry.set_active(Some(k.output_handle));
        }
        self.keyboard_focus_key = key;
    }

    pub fn get_output_by_handle_mut(
        &mut self,
        handle: OutputHandle,
    ) -> Option<&mut PerOutputSurface> {
        self.get_first_surface_for_output_mut(handle)
    }

    pub fn get_output_info(&self, handle: OutputHandle) -> Option<&OutputInfo> {
        self.output_registry.get(handle)
    }

    pub fn get_output_info_mut(&mut self, handle: OutputHandle) -> Option<&mut OutputInfo> {
        self.output_registry.get_mut(handle)
    }

    pub fn all_output_info(&self) -> impl Iterator<Item = &OutputInfo> {
        self.output_registry.all_info()
    }

    pub const fn output_registry(&self) -> &OutputRegistry {
        &self.output_registry
    }

    pub fn shell_surface_names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self
            .surface_handle_to_name
            .values()
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    pub fn surfaces_by_name(&self, surface_name: &str) -> Vec<&PerOutputSurface> {
        let matching_handles: Vec<SurfaceHandle> = self
            .surface_handle_to_name
            .iter()
            .filter(|(_, n)| n.as_str() == surface_name)
            .map(|(h, _)| *h)
            .collect();

        self.surfaces
            .iter()
            .filter(|(k, _)| matching_handles.contains(&k.surface_handle))
            .map(|(_, v)| v)
            .collect()
    }

    pub fn surfaces_by_name_mut(&mut self, surface_name: &str) -> Vec<&mut PerOutputSurface> {
        let matching_handles: Vec<SurfaceHandle> = self
            .surface_handle_to_name
            .iter()
            .filter(|(_, n)| n.as_str() == surface_name)
            .map(|(h, _)| *h)
            .collect();

        self.surfaces
            .iter_mut()
            .filter(|(k, _)| matching_handles.contains(&k.surface_handle))
            .map(|(_, v)| v)
            .collect()
    }

    pub fn surfaces_by_handle(&self, handle: SurfaceHandle) -> Vec<&PerOutputSurface> {
        self.surfaces
            .iter()
            .filter(|(k, _)| k.surface_handle == handle)
            .map(|(_, v)| v)
            .collect()
    }

    pub fn surfaces_by_handle_mut(&mut self, handle: SurfaceHandle) -> Vec<&mut PerOutputSurface> {
        self.surfaces
            .iter_mut()
            .filter(|(k, _)| k.surface_handle == handle)
            .map(|(_, v)| v)
            .collect()
    }

    pub fn surfaces_by_name_and_output(
        &self,
        name: &str,
        output: OutputHandle,
    ) -> Vec<&PerOutputSurface> {
        let matching_handles: Vec<SurfaceHandle> = self
            .surface_handle_to_name
            .iter()
            .filter(|(_, n)| n.as_str() == name)
            .map(|(h, _)| *h)
            .collect();

        self.surfaces
            .iter()
            .filter(|(k, _)| {
                k.output_handle == output && matching_handles.contains(&k.surface_handle)
            })
            .map(|(_, v)| v)
            .collect()
    }

    pub fn surfaces_by_name_and_output_mut(
        &mut self,
        name: &str,
        output: OutputHandle,
    ) -> Vec<&mut PerOutputSurface> {
        let matching_handles: Vec<SurfaceHandle> = self
            .surface_handle_to_name
            .iter()
            .filter(|(_, n)| n.as_str() == name)
            .map(|(h, _)| *h)
            .collect();

        self.surfaces
            .iter_mut()
            .filter(|(k, _)| {
                k.output_handle == output && matching_handles.contains(&k.surface_handle)
            })
            .map(|(_, v)| v)
            .collect()
    }

    pub fn get_output_by_handle(&self, handle: OutputHandle) -> Option<&PerOutputSurface> {
        self.get_first_surface_for_output(handle)
    }

    pub fn outputs_with_handles(&self) -> impl Iterator<Item = (OutputHandle, &PerOutputSurface)> {
        self.surfaces
            .iter()
            .map(|(key, surface)| (key.output_handle, surface))
    }

    pub fn outputs_with_info(&self) -> impl Iterator<Item = (&OutputInfo, &PerOutputSurface)> {
        self.output_registry.all_info().filter_map(|info| {
            let handle = info.handle();
            self.get_first_surface_for_output(handle)
                .map(|surface| (info, surface))
        })
    }

    pub fn all_surfaces_for_output_mut(
        &mut self,
        output_id: &ObjectId,
    ) -> Vec<&mut PerOutputSurface> {
        let Some(handle) = self.output_mapping.get(output_id) else {
            return Vec::new();
        };

        self.surfaces
            .iter_mut()
            .filter(|(k, _)| k.output_handle == handle)
            .map(|(_, v)| v)
            .collect()
    }

    pub fn remove_surfaces_by_name(&mut self, surface_name: &str) -> Vec<PerOutputSurface> {
        let matching_handles: Vec<SurfaceHandle> = self
            .surface_handle_to_name
            .iter()
            .filter(|(_, n)| n.as_str() == surface_name)
            .map(|(h, _)| *h)
            .collect();

        let keys_to_remove: Vec<_> = self
            .surfaces
            .keys()
            .filter(|k| matching_handles.contains(&k.surface_handle))
            .cloned()
            .collect();

        let mut removed = Vec::new();
        for key in keys_to_remove {
            if let Some(surface) = self.surfaces.remove(&key) {
                removed.push(surface);
            }
        }

        self.surface_to_key
            .retain(|_, k| !matching_handles.contains(&k.surface_handle));

        removed
    }

    pub fn create_region(&self) -> Option<WlRegion> {
        let queue = self.queue_handle.as_ref()?;
        let ctx = self.global_context.as_ref()?;
        let region = ctx.compositor.create_region(queue, ());
        Some(region)
    }
}

impl RenderableSet for AppState {
    fn render_all_dirty(&self) -> Result<()> {
        for surface in self.all_outputs() {
            surface
                .window()
                .render_frame_if_dirty()
                .map_err(|e| RenderingError::Operation {
                    message: e.to_string(),
                })?;
        }
        Ok(())
    }
}
