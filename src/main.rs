extern crate rand;
extern crate kdtree;
extern crate gfx_core;
extern crate env_logger;
extern crate piston_window;
#[macro_use] extern crate log;
#[macro_use] extern crate clap;

use std::{io, iter, process};
use std::path::PathBuf;
use std::cmp::Ordering;

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

    let mut obstacles = Vec::new();
    let mut env = Env::new();

    loop {
        let mut action: Box<FnMut(&mut Vec<Segment>)> = {
            let mut cutter = VisualCutter::new(&obstacles);
            let _tree = kdtree::KdvTree::build(
                iter::once(Axis::X).chain(iter::once(Axis::Y)),
                0 .. obstacles.len(),
                &mut cutter,
            ).unwrap_or_else(|()| unreachable!());

            loop {
                let event = if let Some(ev) = window.next() {
                    ev
                } else {
                    return Ok(());
                };
                let maybe_result = window.draw_2d(&event, |context, g2d| {
                    use piston_window::{clear, text, ellipse, line, Transformed};
                    // clear everything
                    clear([0.0, 0.0, 0.0, 1.0], g2d);

                    // draw kdtree cuts mesh
                    for &(ref cut_seg, ref axis) in cutter.cuts.iter() {
                        let color = match axis {
                            &Axis::X => [0.25, 0.25, 0., 1.0],
                            &Axis::Y => [0., 0.25, 0.25, 1.0],
                        };
                        line(color, 1., [cut_seg.src.x, cut_seg.src.y, cut_seg.dst.x, cut_seg.dst.y], context.transform, g2d);
                    }
                    // draw obstacles
                    for &Segment { src: Point { x: mx, y: my, }, dst: Point { x: cx, y: cy, }, } in obstacles.iter() {
                        line([0.75, 0., 0., 1.0], 2., [cx, cy, mx, my], context.transform, g2d);
                    }
                    // draw cursor
                    if let Some(Point { x: mx, y: my, }) = env.cursor {
                        let color = match env.business {
                            Business::Construct =>
                                [1.0, 0., 0., 1.0],
                            Business::Collide =>
                                [0., 1.0, 0., 1.0],
                        };
                        if let Some(Point { x: cx, y: cy, }) = env.obj_start {
                            line(color, 3., [cx, cy, mx, my], context.transform, g2d);
                        } else {
                            ellipse(
                                color,
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
                        return Ok(()),
                    Event::Input(Input::Button(ButtonArgs { button: Button::Keyboard(Key::C), state: ButtonState::Release, .. })) =>
                        break Box::new(|obstacles| {
                            obstacles.clear();
                            env.reset_cursor();
                        }),
                    Event::Input(Input::Button(ButtonArgs { button: Button::Keyboard(Key::M), state: ButtonState::Release, .. })) =>
                        env.toggle_mode(),
                    Event::Input(Input::Move(Motion::MouseCursor(x, y))) =>
                        env.set_cursor(x, y),
                    Event::Input(Input::Cursor(false)) =>
                        env.reset_cursor(),
                    Event::Input(Input::Button(ButtonArgs { button: Button::Mouse(MouseButton::Left), state: ButtonState::Release, .. })) =>
                        break Box::new(|obstacles| env.toggle_obj(obstacles)),
                    Event::Input(Input::Resize(width, height)) =>
                        env.reset(width, height),
                    _ =>
                        (),
                }
            }
        };
        action(&mut obstacles);
    }
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
}

impl Env {
    fn new() -> Env {
        Env {
            business: Business::Construct,
            cursor: None,
            obj_start: None,
        }
    }

    fn reset(&mut self, _width: u32, _height: u32) {
        self.reset_cursor();
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

    fn toggle_obj(&mut self, obstacles: &mut Vec<Segment>) {
        if let Some(src) = self.cursor {
            self.obj_start = if let Some(dst) = self.obj_start {
                match self.business {
                    Business::Construct =>
                        obstacles.push(Segment { src, dst, }),
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

    fn toggle_mode(&mut self) {
        self.business = match self.business {
            Business::Construct =>
                Business::Collide,
            Business::Collide =>
                Business::Construct,
        };
    }
}

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    src: Point,
    dst: Point,
}

#[derive(Clone, Debug)]
enum Axis { X, Y, }

impl kdtree::Axis<Point> for Axis {
    fn cmp_points(&self, a: &Point, b: &Point) -> Ordering {
        match self {
            &Axis::X =>
                if a.x < b.x { Ordering::Less } else if a.x > b.x { Ordering::Greater } else { Ordering::Equal },
            &Axis::Y =>
                if a.y < b.y { Ordering::Less } else if a.y > b.y { Ordering::Greater } else { Ordering::Equal },
        }
    }
}

#[derive(Clone, Debug)]
struct Bound {
    lt: Point,
    rb: Point,
}

impl kdtree::BoundingVolume for Bound {
    type Point = Point;

    fn min_corner(&self) -> Self::Point { self.lt }
    fn max_corner(&self) -> Self::Point { self.rb }
}

struct VisualCutter<'a> {
    shapes: &'a [Segment],
    cuts: Vec<(Segment, Axis)>,
}

impl<'a> VisualCutter<'a> {
    fn new(shapes: &'a [Segment]) -> VisualCutter<'a> {
        VisualCutter {
            shapes,
            cuts: Vec::new(),
        }
    }
}

impl<'a> kdtree::VolumeManager<usize, Axis> for VisualCutter<'a> {
    type BoundingVolume = Bound;
    type Error = ();

    fn bounding_volume(&self, &shape_index: &usize) -> Self::BoundingVolume {
        let shape = &self.shapes[shape_index];
        Bound {
            lt: Point {
                x: if shape.src.x < shape.dst.x { shape.src.x } else { shape.dst.x },
                y: if shape.src.y < shape.dst.y { shape.src.y } else { shape.dst.y },
            },
            rb: Point {
                x: if shape.src.x > shape.dst.x { shape.src.x } else { shape.dst.x },
                y: if shape.src.y > shape.dst.y { shape.src.y } else { shape.dst.y },
            },
        }
    }

    fn cut_point<I>(&mut self, cut_axis: &Axis, points: I) -> Option<Point> where I: Iterator<Item = Point> {
        let mut point_min = None;
        let mut point_max = None;
        let mut point_sum = Point { x: 0., y: 0., };
        let mut total = 0;
        for p in points {
            let pmin = point_min.get_or_insert(p);
            if p.x < pmin.x { pmin.x = p.x; }
            if p.y < pmin.y { pmin.y = p.y; }
            let pmax = point_max.get_or_insert(p);
            if p.x > pmax.x { pmax.x = p.x; }
            if p.y > pmax.y { pmax.y = p.y; }
            point_sum.x += p.x;
            point_sum.y += p.y;
            total += 1;
        }
        if total == 0 {
            None
        } else {
            let point_mid = Point {
                x: point_sum.x / total as f64,
                y: point_sum.y / total as f64,
            };
            if let (Some(pmin), Some(pmax)) = (point_min, point_max) {
                let cut_seg = match cut_axis {
                    &Axis::X => Segment {
                        src: Point { x: point_mid.x, y: pmin.y, },
                        dst: Point { x: point_mid.x, y: pmax.y, },
                    },
                    &Axis::Y => Segment {
                        src: Point { x: pmin.x, y: point_mid.y, },
                        dst: Point { x: pmax.x, y: point_mid.y, },
                    },
                };
                self.cuts.push((cut_seg, cut_axis.clone()));
            }
            Some(point_mid)
        }
    }

    fn cut(&mut self, &shape_index: &usize, fragment: &Bound, cut_axis: &Axis, cut_point: &Point) ->
        Result<Option<(Bound, Bound)>, Self::Error>
    {
        let shape = &self.shapes[shape_index];
        match cut_axis {
            &Axis::X => if cut_point.x >= fragment.lt.x && cut_point.x <= fragment.rb.x {
                if fragment.rb.x - fragment.lt.x < 10. {
                    Ok(None)
                } else {
                    let factor = (cut_point.x - shape.src.x) / (shape.dst.x - shape.src.x);
                    let y = shape.src.y + (factor * (shape.dst.y - shape.src.y));
                    let left_point = if shape.src.x < shape.dst.x { shape.src } else { shape.dst };
                    let left_bound = Bound {
                        lt: Point {
                            x: fragment.lt.x,
                            y: if left_point.y < cut_point.y { fragment.lt.y } else { y },
                        },
                        rb: Point {
                            x: cut_point.x,
                            y: if left_point.y < cut_point.y { y } else { fragment.rb.y },
                        }
                    };
                    let right_point = if shape.src.x < shape.dst.x { shape.dst } else { shape.src };
                    let right_bound = Bound {
                        lt: Point {
                            x: cut_point.x,
                            y: if right_point.y < cut_point.y { fragment.lt.y } else { y },
                        },
                        rb: Point {
                            x: fragment.rb.x,
                            y: if right_point.y < cut_point.y { y } else { fragment.rb.y },
                        },
                    };
                    Ok(Some((left_bound, right_bound)))
                }
            } else {
                return Ok(None);
            },
            &Axis::Y => if cut_point.y >= fragment.lt.y && cut_point.y <= fragment.rb.y {
                if fragment.rb.y - fragment.lt.y < 10. {
                    Ok(None)
                } else {
                    let factor = (cut_point.y - shape.src.y) / (shape.dst.y - shape.src.y);
                    let x = shape.src.x + (factor * (shape.dst.x - shape.src.x));
                    let upper_point = if shape.src.y < shape.dst.y { shape.src } else { shape.dst };
                    let upper_bound = Bound {
                        lt: Point {
                            x: if upper_point.x < cut_point.x { fragment.lt.x } else { x },
                            y: fragment.lt.y,
                        },
                        rb: Point {
                            x: if upper_point.x < cut_point.x { x } else { fragment.rb.x },
                            y: cut_point.y,
                        }
                    };
                    let lower_point = if shape.src.y < shape.dst.y { shape.dst } else { shape.src };
                    let lower_bound = Bound {
                        lt: Point {
                            x: if lower_point.x < cut_point.x { fragment.lt.x } else { x },
                            y: cut_point.y,
                        },
                        rb: Point {
                            x: if lower_point.x < cut_point.x { x } else { fragment.rb.x },
                            y: fragment.rb.y,
                        },
                    };
                    Ok(Some((upper_bound, lower_bound)))
                }
            } else {
                return Ok(None);
            },
        }
    }
}
