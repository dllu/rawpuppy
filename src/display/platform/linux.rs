use super::super::{Icc, Request, Resolved, overlap};
use anyhow::{Result, ensure};
use x11rb::{
    connection::Connection,
    protocol::{
        randr::ConnectionExt as _,
        xinerama::ConnectionExt as _,
        xproto::{AtomEnum, ConnectionExt as _},
    },
    rust_connection::RustConnection,
};

fn property(conn: &RustConnection, window: u32, name: &str) -> Result<Option<Vec<u8>>> {
    let atom = conn.intern_atom(true, name.as_bytes())?.reply()?.atom;
    if atom == 0 {
        return Ok(None);
    }
    let reply = conn
        .get_property(false, window, atom, AtomEnum::CARDINAL, 0, u32::MAX)?
        .reply()?;
    if reply.type_ == 0 {
        return Ok(None);
    }
    ensure!(
        reply.format == 8 && reply.bytes_after == 0,
        "Invalid or incomplete X11 display profile property"
    );
    Ok(Some(reply.value))
}
fn resolved(bytes: Vec<u8>, label: String) -> Result<Resolved> {
    Ok(Resolved {
        icc: Some(Icc::from_bytes(bytes)?),
        label,
    })
}

pub fn x11(request: &Request) -> Result<Option<Resolved>> {
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let mut primary = true;
    let monitors = conn
        .randr_get_monitors(root, true)
        .ok()
        .and_then(|v| v.reply().ok());
    if let Some(monitors) = &monitors {
        let selected = monitors.monitors.iter().max_by_key(|m| {
            request
                .monitor
                .as_ref()
                .map_or(if m.primary { 1 } else { 0 }, |wanted| {
                    overlap(
                        wanted.rect,
                        [m.x.into(), m.y.into(), m.width.into(), m.height.into()],
                    )
                })
        });
        if let Some(monitor) = selected {
            primary = monitor.primary || monitors.monitors.len() == 1;
            for output in &monitor.outputs {
                let atom = conn.intern_atom(true, b"_ICC_PROFILE")?.reply()?.atom;
                if atom != 0
                    && let Ok(reply) = conn
                        .randr_get_output_property(
                            *output,
                            atom,
                            AtomEnum::CARDINAL,
                            0,
                            u32::MAX,
                            false,
                            false,
                        )?
                        .reply()
                    && reply.format == 8
                    && !reply.data.is_empty()
                {
                    ensure!(reply.bytes_after == 0, "Incomplete RandR display profile");
                    return resolved(reply.data, "Automatic: RandR monitor ICC".into()).map(Some);
                }
            }
        }
    }
    let index = conn
        .xinerama_query_screens()
        .ok()
        .and_then(|v| v.reply().ok())
        .and_then(|r| {
            request.monitor.as_ref().and_then(|wanted| {
                r.screen_info.iter().position(|m| {
                    wanted.rect
                        == [
                            i32::from(m.x_org),
                            i32::from(m.y_org),
                            i32::from(m.width),
                            i32::from(m.height),
                        ]
                })
            })
        });
    if let Some(index) = index
        && index > 0
    {
        if let Some(bytes) = property(&conn, root, &format!("_ICC_PROFILE_{index}"))? {
            return resolved(bytes, format!("Automatic: X11 monitor {index} ICC")).map(Some);
        }
    } else if primary && let Some(bytes) = property(&conn, root, "_ICC_PROFILE")? {
        return resolved(bytes, "Automatic: X11 monitor ICC".into()).map(Some);
    }
    colord(request)
}

struct WaylandProbe;
impl
    wayland_client::Dispatch<
        wayland_client::protocol::wl_registry::WlRegistry,
        wayland_client::globals::GlobalListContents,
    > for WaylandProbe
{
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_registry::WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &wayland_client::Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}
pub fn wayland_managed() -> bool {
    let Ok(connection) = wayland_client::Connection::connect_to_env() else {
        return false;
    };
    let Ok((globals, _queue)) =
        wayland_client::globals::registry_queue_init::<WaylandProbe>(&connection)
    else {
        return false;
    };
    globals
        .contents()
        .with_list(|items| items.iter().any(|g| g.interface == "wp_color_manager_v1"))
}
pub fn wayland(request: &Request) -> Result<Resolved> {
    if wayland_managed() {
        return Ok(Resolved {
            icc: None,
            label: "Automatic: compositor sRGB".into(),
        });
    }
    Ok(colord(request)?.unwrap_or(Resolved {
        icc: None,
        label: "Automatic: legacy Wayland sRGB fallback".into(),
    }))
}

fn colord(request: &Request) -> Result<Option<Resolved>> {
    let Ok(connection) = zbus::blocking::Connection::system() else {
        return Ok(None);
    };
    let Ok(manager) = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.ColorManager",
        "/org/freedesktop/ColorManager",
        "org.freedesktop.ColorManager",
    ) else {
        return Ok(None);
    };
    let Ok(devices) = manager
        .call::<_, _, Vec<zbus::zvariant::OwnedObjectPath>>("GetDevicesByKind", &("display",))
    else {
        return Ok(None);
    };
    for path in devices {
        let device = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.ColorManager",
            path,
            "org.freedesktop.ColorManager.Device",
        )?;
        let metadata =
            device.get_property::<std::collections::HashMap<String, String>>("Metadata")?;
        let matched = request
            .monitor
            .as_ref()
            .and_then(|m| m.name.as_ref())
            .is_some_and(|name| metadata.get("XRANDR_name") == Some(name));
        if !matched {
            continue;
        }
        let profiles = device.get_property::<Vec<zbus::zvariant::OwnedObjectPath>>("Profiles")?;
        if let Some(profile) = profiles.first() {
            let profile = zbus::blocking::Proxy::new(
                &connection,
                "org.freedesktop.ColorManager",
                profile,
                "org.freedesktop.ColorManager.Profile",
            )?;
            let filename = profile.get_property::<String>("Filename")?;
            if !filename.is_empty() {
                return resolved(
                    std::fs::read(filename)?,
                    "Automatic: colord monitor ICC".into(),
                )
                .map(Some);
            }
        }
    }
    Ok(None)
}
