//! Explicit SDR source description on the owned native Wayland window.
use anyhow::{Context, Result, ensure};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::{
    any::Any,
    collections::BTreeSet,
    io::Write,
    os::fd::AsFd,
    sync::Arc,
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum,
    protocol::{wl_callback, wl_registry, wl_surface},
};
use wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_surface_v1::WpColorManagementSurfaceV1,
    wp_color_manager_v1::{
        self, Feature, Primaries, RenderIntent, TransferFunction, WpColorManagerV1,
    },
    wp_image_description_creator_icc_v1::WpImageDescriptionCreatorIccV1,
    wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1,
    wp_image_description_v1::{self, WpImageDescriptionV1},
};

#[derive(Default)]
struct State {
    global: Option<(u32, u32)>,
    registry_done: bool,
    capabilities_done: bool,
    features: BTreeSet<u32>,
    primaries: BTreeSet<u32>,
    transfers: BTreeSet<u32>,
    intents: BTreeSet<u32>,
    ready: bool,
    failure: Option<String>,
}
impl State {
    fn parametric_srgb(&self) -> bool {
        self.features.contains(&(Feature::Parametric as u32))
            && self.primaries.contains(&(Primaries::Srgb as u32))
            && self.transfers.contains(&(TransferFunction::Srgb as u32))
    }
    fn render_intent(&self) -> Result<RenderIntent> {
        if self.intents.contains(&(RenderIntent::Relative as u32)) {
            Ok(RenderIntent::Relative)
        } else if self.intents.contains(&(RenderIntent::Perceptual as u32)) {
            Ok(RenderIntent::Perceptual)
        } else {
            anyhow::bail!("Wayland compositor has no supported SDR render intent")
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Globals,
    Capabilities,
    Description,
    Tagged,
    Unavailable,
    Failed,
}

pub struct WaylandSurface {
    phase: Phase,
    state: State,
    started: Instant,
    surface_id: wayland_backend::sys::client::ObjectId,
    control: Option<WpColorManagementSurfaceV1>,
    manager: Option<WpColorManagerV1>,
    description: Option<WpImageDescriptionV1>,
    registry: wl_registry::WlRegistry,
    connection: Connection,
    queue: EventQueue<State>,
    // Last field keeps the borrowed wl_display/wl_surface alive until all of
    // our owned protocol objects and the guest backend have been cleaned up.
    _window: Arc<dyn Any>,
}

impl WaylandSurface {
    pub fn bind<W: HasDisplayHandle + HasWindowHandle + 'static>(
        window: Arc<W>,
    ) -> Result<Option<Self>> {
        let RawDisplayHandle::Wayland(display) = window.display_handle()?.as_raw() else {
            return Ok(None);
        };
        let RawWindowHandle::Wayland(surface) = window.window_handle()?.as_raw() else {
            return Ok(None);
        };
        // SAFETY: guest mode creates only our own proxies and never closes
        // Winit's connection. The retained window keeps these handles live.
        let backend = unsafe {
            wayland_backend::sys::client::Backend::from_foreign_display(
                display.display.as_ptr().cast(),
            )
        };
        let connection = Connection::from_backend(backend);
        let queue = connection.new_event_queue::<State>();
        let handle = queue.handle();
        let registry = connection.display().get_registry(&handle, ());
        connection.display().sync(&handle, ());
        // SAFETY: borrow only the live identifier; no listener/queue ownership
        // is changed, and this module never destroys or commits Winit's surface.
        let surface_id = unsafe {
            wayland_backend::sys::client::ObjectId::from_ptr(
                wl_surface::WlSurface::interface(),
                surface.surface.as_ptr().cast(),
            )
        }?;
        let binding = Self {
            phase: Phase::Globals,
            state: State::default(),
            started: Instant::now(),
            surface_id,
            control: None,
            manager: None,
            description: None,
            registry,
            connection,
            queue,
            _window: window,
        };
        binding.connection.flush()?;
        Ok(Some(binding))
    }
    pub fn pending(&self) -> bool {
        matches!(
            self.phase,
            Phase::Globals | Phase::Capabilities | Phase::Description
        )
    }
    pub fn tagged(&self) -> bool {
        self.phase == Phase::Tagged
    }
    /// Winit reads the shared socket. Dispatch only our private pending queue.
    pub fn poll(&mut self) -> Result<()> {
        if self.phase == Phase::Failed {
            return Ok(());
        }
        let result = self.advance();
        if result.is_err() {
            self.phase = Phase::Failed;
        }
        result
    }
    fn advance(&mut self) -> Result<()> {
        self.queue.dispatch_pending(&mut self.state)?;
        if !self.pending() {
            return Ok(());
        }
        ensure!(
            self.started.elapsed() < Duration::from_secs(3),
            "Wayland surface color negotiation timed out"
        );
        let handle = self.queue.handle();
        if self.phase == Phase::Globals && self.state.registry_done {
            let Some((name, version)) = self.state.global else {
                self.phase = Phase::Unavailable;
                return Ok(());
            };
            self.manager = Some(self.registry.bind(name, version.min(1), &handle, ()));
            self.phase = Phase::Capabilities;
            self.connection.flush()?;
        }
        if self.phase == Phase::Capabilities && self.state.capabilities_done {
            let parametric = self.state.parametric_srgb();
            if parametric {
                let creator = self
                    .manager
                    .as_ref()
                    .unwrap()
                    .create_parametric_creator(&handle, ());
                creator.set_primaries_named(Primaries::Srgb);
                creator.set_tf_named(TransferFunction::Srgb);
                self.description = Some(creator.create(&handle, ()));
            } else {
                ensure!(
                    self.state.features.contains(&(Feature::IccV2V4 as u32)),
                    "Wayland compositor cannot describe this sRGB surface"
                );
                let bytes = crate::export::profile(crate::color::OutputSpace::Srgb)?.icc()?;
                let mut file = tempfile::tempfile()?;
                file.write_all(&bytes)?;
                let creator = self
                    .manager
                    .as_ref()
                    .unwrap()
                    .create_icc_creator(&handle, ());
                creator.set_icc_file(file.as_fd(), 0, bytes.len().try_into()?);
                self.description = Some(creator.create(&handle, ()));
                // Requests duplicate the FD; flush before releasing our file.
                self.connection.flush()?;
            }
            self.phase = Phase::Description;
            self.connection.flush()?;
        }
        if self.phase == Phase::Description && (self.state.ready || self.state.failure.is_some()) {
            ensure!(
                self.state.ready,
                "{}",
                self.state
                    .failure
                    .as_deref()
                    .unwrap_or("Wayland sRGB description is not ready")
            );
            let intent = self.state.render_intent()?;
            let borrowed =
                wl_surface::WlSurface::from_id(&self.connection, self.surface_id.clone())?;
            let control = self
                .manager
                .as_ref()
                .unwrap()
                .get_surface(&borrowed, &handle, ());
            control.set_image_description(self.description.as_ref().unwrap(), intent);
            self.control = Some(control);
            self.description.take().unwrap().destroy();
            self.connection
                .flush()
                .context("Setting owned Wayland surface to sRGB")?;
            self.phase = Phase::Tagged;
        }
        Ok(())
    }
}
impl Drop for WaylandSurface {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            control.destroy();
        }
        if let Some(description) = self.description.take() {
            description.destroy();
        }
        if let Some(manager) = self.manager.take() {
            manager.destroy();
        }
        let _ = self
            .connection
            .backend()
            .destroy_object(&self.registry.id());
        let _ = self.connection.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == "wp_color_manager_v1"
        {
            state.global = Some((name, version));
        }
    }
}
impl Dispatch<wl_callback::WlCallback, ()> for State {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.registry_done = true;
    }
}
impl Dispatch<WpColorManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpColorManagerV1,
        event: wp_color_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_color_manager_v1::Event::SupportedFeature {
                feature: WEnum::Value(value),
            } => {
                state.features.insert(value as u32);
            }
            wp_color_manager_v1::Event::SupportedPrimariesNamed {
                primaries: WEnum::Value(value),
            } => {
                state.primaries.insert(value as u32);
            }
            wp_color_manager_v1::Event::SupportedTfNamed {
                tf: WEnum::Value(value),
            } => {
                state.transfers.insert(value as u32);
            }
            wp_color_manager_v1::Event::SupportedIntent {
                render_intent: WEnum::Value(value),
            } => {
                state.intents.insert(value as u32);
            }
            wp_color_manager_v1::Event::Done => state.capabilities_done = true,
            _ => {}
        }
    }
}
impl Dispatch<WpImageDescriptionV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &WpImageDescriptionV1,
        event: wp_image_description_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_image_description_v1::Event::Ready { .. } => state.ready = true,
            wp_image_description_v1::Event::Failed { cause, msg } => {
                state.failure = Some(format!("{cause:?}: {msg}"))
            }
            _ => {}
        }
    }
}
wayland_client::delegate_noop!(State: ignore WpImageDescriptionCreatorParamsV1);
wayland_client::delegate_noop!(State: ignore WpImageDescriptionCreatorIccV1);
wayland_client::delegate_noop!(State: ignore WpColorManagementSurfaceV1);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn description_and_intent_never_use_unadvertised_capabilities() {
        let mut state = State::default();
        assert!(!state.parametric_srgb());
        assert!(state.render_intent().is_err());
        state.features.insert(Feature::Parametric as u32);
        state.primaries.insert(Primaries::Srgb as u32);
        state.transfers.insert(TransferFunction::Gamma22 as u32);
        assert!(!state.parametric_srgb());
        state.transfers.insert(TransferFunction::Srgb as u32);
        assert!(state.parametric_srgb());
        state.intents.insert(RenderIntent::Perceptual as u32);
        assert_eq!(state.render_intent().unwrap(), RenderIntent::Perceptual);
        state.intents.insert(RenderIntent::Relative as u32);
        assert_eq!(state.render_intent().unwrap(), RenderIntent::Relative);
    }
}
