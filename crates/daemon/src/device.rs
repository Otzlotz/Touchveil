//! Find the touchscreen and create the virtual device through which normal touches reach
//! the applications.

use crate::filter::Filter;
use crate::router::is_mt;
use evdev::uinput::{VirtualDevice, VirtualDeviceBuilder};
use evdev::{
    AbsInfo, AbsoluteAxisType, AttributeSet, BusType, Device, InputId, Key, PropType,
    UinputAbsSetup,
};
use std::io::{self, ErrorKind};
use std::os::fd::RawFd;
use std::path::PathBuf;

const PROXY_NAME: &str = "Touchveil virtual touchscreen";
/// ID of the virtual device, so the daemon never takes it over itself.
const PROXY_VENDOR: u16 = 0x7476; // "tv"
const PROXY_PRODUCT: u16 = 0x0001;

fn is_proxy(dev: &Device) -> bool {
    let id = dev.input_id();
    dev.name() == Some(PROXY_NAME) || (id.vendor() == PROXY_VENDOR && id.product() == PROXY_PRODUCT)
}

/// Multitouch devices whose name matches the filter. Without a filter only touchscreens
/// (INPUT_PROP_DIRECT), so no touchpad gets caught.
pub(crate) fn find_devices(filter: Option<&Filter>) -> Vec<(PathBuf, Device)> {
    let mut found: Vec<(PathBuf, Device)> = evdev::enumerate()
        .filter(|(_, dev)| {
            let has_mt = dev
                .supported_absolute_axes()
                .is_some_and(|a| a.contains(AbsoluteAxisType::ABS_MT_POSITION_X));
            let wanted = match filter {
                Some(f) => f.matches(dev.name().unwrap_or("")),
                None => dev.properties().contains(PropType::DIRECT),
            };
            has_mt && wanted && !is_proxy(dev)
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Error message when no matching device was found. Distinguishes missing read access
/// (enumerate skips devices it may not open) from a wrong name.
pub(crate) fn not_found_error(filter: Option<&str>) -> io::Error {
    let nodes = std::fs::read_dir("/dev/input").map_or(0, |d| {
        d.flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("event"))
            .count()
    });
    let readable: Vec<String> = evdev::enumerate()
        .filter(|(_, d)| !is_proxy(d))
        .map(|(_, d)| d.name().unwrap_or("?").to_string())
        .collect();
    let msg = if readable.is_empty() && nodes > 0 {
        "No read access to /dev/input. Run ./install.sh once (udev rule).".to_string()
    } else if readable.is_empty() {
        "No touchscreen found, there are no input devices.".to_string()
    } else {
        let what = filter.map_or("No touchscreen".to_string(), |f| {
            format!("No device matches device = \"{f}\"")
        });
        format!("{what}. Readable devices: {}", readable.join(", "))
    };
    io::Error::new(ErrorKind::NotFound, msg)
}

pub(crate) fn build_virtual(src: &Device) -> io::Result<VirtualDevice> {
    let abs_state = src.get_abs_state()?;
    let mut keys = AttributeSet::<Key>::new();
    keys.insert(Key::BTN_TOUCH);
    let mut b = VirtualDeviceBuilder::new()?
        .name(PROXY_NAME)
        .input_id(InputId::new(
            BusType::BUS_VIRTUAL,
            PROXY_VENDOR,
            PROXY_PRODUCT,
            1,
        ))
        .with_keys(&keys)?;
    if let Some(axes) = src.supported_absolute_axes() {
        for axis in axes.iter().filter(|a| is_mt(a.0)) {
            let i = abs_state[axis.0 as usize];
            let info = AbsInfo::new(i.value, i.minimum, i.maximum, i.fuzz, i.flat, i.resolution);
            b = b.with_absolute_axis(&UinputAbsSetup::new(axis, info))?;
        }
    }
    // Copy INPUT_PROP_DIRECT, otherwise libinput does not recognize the device as a touchscreen.
    b = b.with_properties(src.properties())?;
    b.build()
}

pub(crate) fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
