extern crate rand;
extern crate kdtree;
extern crate gfx_core;
extern crate env_logger;
extern crate piston_window;
#[macro_use] extern crate log;
#[macro_use] extern crate clap;

use std::{io, process};
// use std::{io, thread, process};
// use std::sync::mpsc;
use std::path::PathBuf;

use clap::Arg;
use piston_window::{
    OpenGL,
    PistonWindow,
    WindowSettings,
    TextureSettings,
    Glyphs,
    Event,
    Input,
    Button,
    ButtonArgs,
    ButtonState,
    MouseButton,
    Motion,
    Key,
};

mod common;

use common::{Point, Segment};

fn main() {
    env_logger::init();
    match run() {
        Ok(()) =>
            info!("graceful shutdown"),
        Err(e) => {
            error!("fatal error: {:?}", e);
            process::exit(1);
        },
    }
}

#[derive(Debug)]
enum Error {
    MissingParameter(&'static str),
    Piston(PistonError),
    // ThreadSpawn(io::Error),
    // ThreadJoin(Box<std::any::Any + Send + 'static>),
}

#[derive(Debug)]
enum PistonError {
    BuildWindow(String),
    LoadFont { file: String, error: io::Error, },
    DrawText(gfx_core::factory::CombinedError),
}

const CONSOLE_HEIGHT: u32 = 32;
const SCREEN_WIDTH: u32 = 640;
const SCREEN_HEIGHT: u32 = 480;

fn run() -> Result<(), Error> {
    let matches = app_from_crate!()
        .arg(Arg::with_name("assets-dir")
             .short("a")
             .long("assets-dir")
             .value_name("DIR")
             .help("Graphics resources directory")
             .default_value("./assets")
             .takes_value(true))
        .get_matches();

    let assets_dir = matches.value_of("assets-dir")
        .ok_or(Error::MissingParameter("assets-dir"))?;

    let opengl = OpenGL::V4_1;
    let mut window: PistonWindow = WindowSettings::new("KD-Tree demo", [SCREEN_WIDTH, SCREEN_HEIGHT])
        .exit_on_esc(true)
        .opengl(opengl)
        .build()
        .map_err(PistonError::BuildWindow)
        .map_err(Error::Piston)?;

    let mut font_path = PathBuf::from(assets_dir);
    font_path.push("FiraSans-Regular.ttf");
    let mut glyphs = Glyphs::new(&font_path, window.factory.clone(), TextureSettings::new())
        .map_err(|e| Error::Piston(PistonError::LoadFont {
            file: font_path.to_string_lossy().to_string(),
            error: e,
        }))?;

    let mut env = Env::new();
    while let Some(event) = window.next() {
        let maybe_result = window.draw_2d(&event, |context, g2d| {
            use piston_window::{clear, text, ellipse, line, Transformed};
            // clear everything
            clear([0.0, 0.0, 0.0, 1.0], g2d);

            // draw obstacles
            for &Segment { src: Point { x: mx, y: my, }, dst: Point { x: cx, y: cy, }, } in env.obstacles.iter() {
                line([0.75, 0., 0., 1.0], 2., [cx, cy, mx, my], context.transform, g2d);
            }
            // draw cursor
            if let Some(Point { x: mx, y: my, }) = env.cursor {
                if let Some(Point { x: cx, y: cy, }) = env.obj_start {
                    line([1.0, 0., 0., 1.0], 3., [cx, cy, mx, my], context.transform, g2d);
                } else {
                    ellipse(
                        [1.0, 0., 0., 1.0],
                        [mx - 5., my - 5., 10., 10.,],
                        context.transform,
                        g2d,
                    );
                }
            }
            // draw menu
            text::Text::new_color([0.0, 1.0, 0.0, 1.0], 16).draw(
                &env.business.info_line(),
                &mut glyphs,
                &context.draw_state,
                context.transform.trans(5.0, 20.0),
                g2d
            ).map_err(PistonError::DrawText)?;

            Ok(())
        });
        if let Some(result) = maybe_result {
            let () = result.map_err(Error::Piston)?;
        }

        match event {
            Event::Input(Input::Button(ButtonArgs { button: Button::Keyboard(Key::Q), state: ButtonState::Release, .. })) =>
                break,
            Event::Input(Input::Button(ButtonArgs { button: Button::Keyboard(Key::C), state: ButtonState::Release, .. })) =>
                env.clear(),
            Event::Input(Input::Move(Motion::MouseCursor(x, y))) =>
                env.set_cursor(x, y),
            Event::Input(Input::Cursor(false)) =>
                env.reset_cursor(),
            Event::Input(Input::Button(ButtonArgs { button: Button::Mouse(MouseButton::Left), state: ButtonState::Release, .. })) =>
                env.toggle_obj(),
            Event::Input(Input::Resize(width, height)) =>
                env.reset(width, height),
            _ =>
                (),
        }
    }

    Ok(())
}

enum Business {
    Construct,
    Collide,
}

impl Business {
    fn info_line(&self) -> String {
        match self {
            &Business::Construct =>
                "[ constructing ] <M> switch to collide mode, <C> to clear or <Q> to exit".to_string(),
            &Business::Collide =>
                "[ colliding ] <M> switch to construct mode, <C> to clear or <Q> to exit".to_string(),
        }
    }
}

struct Env {
    business: Business,
    cursor: Option<Point>,
    obj_start: Option<Point>,
    obstacles: Vec<Segment>,
}

impl Env {
    fn new() -> Env {
        Env {
            business: Business::Construct,
            cursor: None,
            obj_start: None,
            obstacles: Vec::new(),
        }
    }

    fn clear(&mut self) {
    }

    fn set_cursor(&mut self, x: f64, y: f64) {
        self.cursor = if y < CONSOLE_HEIGHT as f64 {
            None
        } else {
            Some(Point { x, y, })
        }
    }

    fn reset_cursor(&mut self) {
        self.cursor = None;
        self.obj_start = None;
    }

    fn toggle_obj(&mut self) {
        if let Some(src) = self.cursor {
            self.obj_start = if let Some(dst) = self.obj_start {
                match self.business {
                    Business::Construct =>
                        self.obstacles.push(Segment { src, dst, }),
                    Business::Collide =>
                    // TODO
                        (),
                }
                None
            } else {
                Some(src)
            };
        }
    }

    fn reset(&mut self, _width: u32, _height: u32) {
        self.reset_cursor();
    }
}
