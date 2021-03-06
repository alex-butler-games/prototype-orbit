#![cfg_attr(feature = "bench", feature(test))]
#[cfg(feature = "bench")]
extern crate test;

#[macro_use] extern crate log;
#[macro_use] extern crate gfx;
#[macro_use] extern crate gfx_macros;
#[macro_use] extern crate gfx_shader_watch;
extern crate pretty_env_logger;
extern crate gfx_window_glutin;
extern crate glutin;
extern crate time;
extern crate image;
extern crate cgmath;
extern crate gfx_text;
extern crate easer;
extern crate num;
extern crate uuid;
extern crate rayon;
extern crate single_value_channel;

mod input;
mod state;
mod background;
mod orbitbody;
mod ease;
mod compute;
mod debug;
mod orbitcurve;
mod seer;

use gfx::{Device};
use glutin::*;
use std::io::Cursor;
use std::thread;
use std::time::Duration;
use state::*;
use orbitbody::OrbitBody;

const DESIRED_FPS: u32 = 256;
const DESIRED_DETLA: f64 = 1.0 / DESIRED_FPS as f64;

pub type ColorFormat = gfx::format::Srgba8;
pub type DepthFormat = gfx::format::Depth;


gfx_defines! {
    constant ShaderTime {
        ms_ticks: f32 = "ticks",
    }

    constant UserViewTransform {
        view: [[f32; 4]; 4] = "view",
        proj: [[f32; 4]; 4] = "proj",
    }
}

const CLEAR_COLOR: [f32; 4] = [0.05, 0.05, 0.05, 0.0];

pub fn load_texture<R, F>(factory: &mut F,
                          data: &[u8])
                          -> gfx::handle::ShaderResourceView<R, [f32; 4]>
                          where R: gfx::Resources,
                                F: gfx::Factory<R> {
    use gfx::texture as tex;
    let img = image::load(Cursor::new(data), image::PNG)
        .expect("!image::load")
        .to_rgba();
    let (width, height) = img.dimensions();
    let kind = tex::Kind::D2(width as tex::Size, height as tex::Size, tex::AaMode::Single);
    factory.create_texture_immutable_u8::<ColorFormat>(kind, &[&img])
        .expect("!create_texture_immutable_u8")
        .1
}

pub fn main() {
    pretty_env_logger::init().unwrap();

    let (win_width, win_height) = (1024, 768); // blog size: 800, 478
    let events_loop = EventsLoop::new();
    let builder = WindowBuilder::new()
        .with_title("Orbits".to_string())
        .with_dimensions(win_width, win_height)
        .with_gl_profile(GlProfile::Core)
        .with_gl(GlRequest::Specific(Api::OpenGl, (3, 3)))
        .with_multisampling(0);

    let (window, mut device, mut factory, main_color, main_depth) =
            gfx_window_glutin::init::<ColorFormat, DepthFormat>(builder, &events_loop);

    window.set_position(2560 / 2 + 100, 100); // for development purposes

    let (width_px, height_px) = window.get_inner_size_pixels().unwrap();

    // Compute logic in seperate thread(s)
    let mut state_get = compute::start(State::new(width_px, height_px), events_loop);
    let start = time::precise_time_s();

    // Render logic in main thread
    let mut encoder: gfx::Encoder<_, _> = factory.create_command_buffer().into();
    let mut orbit_body_brush = orbitbody::render::OrbitBodyBrush::new(
        factory.clone(), &main_color, &main_depth);
    let mut background_brush = background::render::BackgroundBrush::new(
        factory.clone(), &main_color, &main_depth);
    let mut debug_info_brush = debug::render::DebugInfoBrush::new(&factory);
    let mut orbit_curve_brushes = Vec::new();

    let (mut delta_sum, mut delta_count) = (0.0, 0);
    let mut passed = time::precise_time_s() - start;

    let mut mean_fps = DESIRED_FPS; // optimistic
    loop {
        let last_passed = passed;
        passed = time::precise_time_s() - start;
        let delta = passed - last_passed;

        let state = state_get.latest();
        if state.user_quit {
            info!("Quitting");
            break;
        }

        let projection = state.projection();
        let view = state.view;
        let visible_world_range = state.visible_world_range();

        encoder.clear(&main_color, CLEAR_COLOR);
        encoder.clear_depth(&main_depth, 1.0);

        let transform = UserViewTransform {
            view: view.into(),
            proj: projection.into(),
        };

        background_brush.draw(&mut encoder, &transform);

        if state.render_curves {
            while state.drawables.orbit_curves.len() > orbit_curve_brushes.len() {
                // add a curve brush per necessary curve, keep them forever
                orbit_curve_brushes.push(orbitcurve::render::OrbitCurveBrush::new(
                        factory.clone(), &main_color, &main_depth));
            }
            for (idx, curve) in state.drawables.orbit_curves.iter().enumerate() {
                orbit_curve_brushes[idx].draw(&mut encoder, &transform, curve, visible_world_range);
            }
        }

        orbit_body_brush.draw(&mut encoder, &transform, &state.drawables.orbit_bodies);


        delta_sum += delta;
        delta_count += 1;
        if delta_sum >= 1.0 { // ie update around every second
            mean_fps = (1.0 / (delta_sum / delta_count as f64)).round() as u32;
            delta_sum = 0.0;
            delta_count = 0;
        }

        debug_info_brush.draw(&mut encoder, &main_color, &state.debug_info.add_render_info(mean_fps))
            .unwrap();
        encoder.flush(&mut device);
        window.swap_buffers().unwrap();
        device.cleanup();


        let frame_time = time::precise_time_s() - start - passed;
        if DESIRED_DETLA - frame_time > 0.0 {
            thread::sleep(Duration::new(0, ((DESIRED_DETLA - frame_time) * 1_000_000_000.0) as u32));
        }
    }
}
