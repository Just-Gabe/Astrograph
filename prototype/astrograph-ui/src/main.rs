use std::num::NonZeroU32;
use std::rc::Rc;

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Default)]
struct App {
    window: Option<Rc<Window>>,
    context: Option<Context<Rc<Window>>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
}

impl App {
    fn draw_once(&mut self, event_loop: &ActiveEventLoop) {
        let window = self.window.as_ref().expect("janela");
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let surface = self.surface.as_mut().expect("surface");
        surface
            .resize(
                NonZeroU32::new(size.width).unwrap(),
                NonZeroU32::new(size.height).unwrap(),
            )
            .expect("resize");
        let mut buffer = surface.buffer_mut().expect("buffer_mut");
        for px in buffer.iter_mut() {
            *px = 0x0024_2630;
        }
        buffer.present().expect("present");
        println!(
            "GATE_OK: janela aberta e frame apresentado ({}x{})",
            size.width, size.height
        );
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Astrograph UI - spike (passo 0)")
            .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0));
        let window = Rc::new(event_loop.create_window(attrs).expect("create_window"));
        let context = Context::new(window.clone()).expect("softbuffer context");
        let surface = Surface::new(&context, window.clone()).expect("softbuffer surface");
        window.request_redraw();
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.draw_once(event_loop),
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("run_app");
    println!("GATE_DONE");
}
