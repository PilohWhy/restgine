use std::sync::Arc;
use winit::dpi::LogicalSize;
use winit::event_loop::EventLoop;
use winit::window::{Window, WindowBuilder};

pub struct WindowContext {
    pub window: Arc<Window>,
    pub event_loop: Option<EventLoop<()>>,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
}

impl WindowContext {
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let event_loop = EventLoop::new()?;
        let window = Arc::new(
            WindowBuilder::new()
                .with_title(title)
                .with_inner_size(LogicalSize::new(width as f64, height as f64))
                .build(&event_loop)?,
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone())?;

        let adapter = pollster_block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or("Failed to find suitable GPU adapter for Restgine surface")?;

        let (device, queue) = pollster_block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Restgine Primary GPU Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoNoVsync, // Uncapped framerate
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Ok(Self {
            window,
            event_loop: Some(event_loop),
            surface,
            device,
            queue,
            config,
        })
    }
}

// Simple synchronous executor for adapter/device initialization
fn pollster_block_on<F: std::future::Future>(future: F) -> F::Output {
    futures_lite_future::block_on(future)
}

mod futures_lite_future {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    pub fn block_on<F: Future>(mut future: F) -> F::Output {
        let mut pinned = unsafe { Pin::new_unchecked(&mut future) };
        let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);

        loop {
            match pinned.as_mut().poll(&mut cx) {
                Poll::Ready(result) => return result,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    static VTABLE: RawWakerVTable = RawWakerVTable::new(
        |_| RawWaker::new(std::ptr::null(), &VTABLE),
        |_| {},
        |_| {},
        |_| {},
    );
}
