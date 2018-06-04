extern crate rand;
extern crate kdvtree;
extern crate gfx_core;
extern crate env_logger;
extern crate piston_window;
#[macro_use] extern crate log;
#[macro_use] extern crate clap;

use std::{io, iter, process};
use std::path::PathBuf;
use std::cmp::Ordering;
use std::collections::HashSet;

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

const KDTREE_CUT_LIMIT: f64 = 32.;
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
    let mut collide_cutter: PointsCutter = Default::default();
    let mut collide_cache = HashSet::new();

    loop {
        let mut action: Box<FnMut(&mut Vec<Segment>)> = {
            let mut visual_cutter = VisualCutter::new();
            let tree = kdvtree::KdvTree::build(
                iter::once(Axis::X).chain(iter::once(Axis::Y)),
                0 .. obstacles.len(),
                cmp_points,
                |&shape_index: &_| get_bounding_volume(&obstacles[shape_index]),
                &mut visual_cutter,
                |&shape_index: &_, fragment: &_, cut_axis: &_, cut_point: &_| {
                    cut_segment_fragment(&obstacles[shape_index], fragment, cut_axis, cut_point)
                },
            ).unwrap_or_else(|()| unreachable!());

            loop {
                let event = if let Some(ev) = window.next() {
                    ev
                } else {
                    return Ok(());
                };
                let maybe_result = window.draw_2d(&event, |context, g2d| {
                    use piston_window::{clear, text, ellipse, line, rectangle, Transformed};
                    // clear everything
                    clear([0.0, 0.0, 0.0, 1.0], g2d);

                    // draw kdtree cuts mesh
                    for &(ref cut_seg, ref axis) in visual_cutter.cuts.iter() {
                        let color = match axis {
                            &Axis::X => [0.25, 0.25, 0., 1.0],
                            &Axis::Y => [0., 0.25, 0.25, 1.0],
                        };
                        line(color, 1., [cut_seg.src.x, cut_seg.src.y, cut_seg.dst.x, cut_seg.dst.y], context.transform, g2d);
                    }
                    // draw collisions or neighbours
                    match (&env.business, env.cursor, env.obj_start) {
                        (&Business::Collide, Some(src), Some(dst)) => {
                            let collide_segment = Segment { src, dst };
                            collide_cache.clear();
                            for maybe_intersection in tree.intersects(
                                &collide_segment,
                                cmp_points,
                                get_bounding_volume,
                                &mut collide_cutter,
                                cut_segment_fragment,
                            )
                            {
                                let kdvtree::Intersection { shape: &shape_index, shape_fragment, needle_fragment } = maybe_intersection
                                    .unwrap_or_else(|()| unreachable!());
                                // highlight collided obstacle
                                if !collide_cache.contains(&shape_index) {
                                    let obstacle = &obstacles[shape_index];
                                    line(
                                        [0.75, 0.75, 0., 1.0],
                                        4.,
                                        [obstacle.src.x, obstacle.src.y, obstacle.dst.x, obstacle.dst.y],
                                        context.transform,
                                        g2d,
                                    );
                                    collide_cache.insert(shape_index);
                                }
                                // show collided obstacle bounding volume
                                rectangle(
                                    [1., 0., 0., 0.5],
                                    [
                                        shape_fragment.lt.x,
                                        shape_fragment.lt.y,
                                        shape_fragment.rb.x - shape_fragment.lt.x,
                                        shape_fragment.rb.y - shape_fragment.lt.y,
                                    ],
                                    context.transform,
                                    g2d,
                                );
                                // show collided user segment bounding volume
                                rectangle(
                                    [0., 1., 0., 0.5],
                                    [
                                        needle_fragment.lt.x,
                                        needle_fragment.lt.y,
                                        needle_fragment.rb.x - needle_fragment.lt.x,
                                        needle_fragment.rb.y - needle_fragment.lt.y,
                                    ],
                                    context.transform,
                                    g2d,
                                );
                            }
                        },
                        (&Business::Neighbours, Some(src), Some(dst)) => {
                            let (width, height) = context.viewport.as_ref()
                                .map(|v| (v.draw_size[0] as f64, v.draw_size[1] as f64))
                                .unwrap_or((SCREEN_WIDTH as f64, SCREEN_HEIGHT as f64));
                            let max_dist = ((width * width) + (height * height)).sqrt();
                            let neighbour_segment = Segment { src, dst };
                            for maybe_neighbour in tree.nearest(
                                &neighbour_segment,
                                cmp_points,
                                get_bounding_volume,
                                cut_segment_fragment,
                                bound_to_cut_point_dist,
                                bound_to_bound_dist,
                            )
                            {
                                let kdvtree::NearestShape { dist, shape: &_shape_index, shape_fragment, } =
                                    maybe_neighbour.unwrap_or_else(|()| unreachable!());
                                let color = if dist < (max_dist * 0.2) {
                                    [1., 1., 1. - (dist / (max_dist * 0.2)) as f32, 1.]
                                } else if dist < (max_dist * 0.4) {
                                    [1., 1. - (dist / (max_dist * 0.4)) as f32, 0., 1.]
                                } else if dist < (max_dist * 0.6) {
                                    [1. - (dist / (max_dist * 0.6)) as f32, 0., 0., 1.]
                                } else {
                                    [0., 0., 0., 1.]
                                };
                                rectangle(
                                    color,
                                    [
                                        shape_fragment.lt.x,
                                        shape_fragment.lt.y,
                                        shape_fragment.rb.x - shape_fragment.lt.x,
                                        shape_fragment.rb.y - shape_fragment.lt.y,
                                    ],
                                    context.transform,
                                    g2d,
                                );
                            }
                        },
                        _ =>
                            (),
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
                                [0., 0.25, 0., 1.0],
                            Business::Neighbours =>
                                [0.824, 0.706, 0.549, 1.0],
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
    Neighbours,
}

impl Business {
    fn info_line(&self) -> String {
        match self {
            &Business::Construct =>
                "[ constructing ] <M> switch to collide mode, <C> to clear or <Q> to exit".to_string(),
            &Business::Collide =>
                "[ colliding ] <M> switch to neighbours mode, <C> to clear or <Q> to exit".to_string(),
            &Business::Neighbours =>
                "[ finding neighbours ] <M> switch to construct mode, <C> to clear or <Q> to exit".to_string(),
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
                    Business::Collide | Business::Neighbours =>
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
                Business::Neighbours,
            Business::Neighbours =>
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

fn cmp_points(axis: &Axis, a: &Point, b: &Point) -> Ordering {
    match axis {
        &Axis::X =>
            if a.x < b.x { Ordering::Less } else if a.x > b.x { Ordering::Greater } else { Ordering::Equal },
        &Axis::Y =>
            if a.y < b.y { Ordering::Less } else if a.y > b.y { Ordering::Greater } else { Ordering::Equal },
    }
}

#[derive(Clone, Debug)]
struct Bound {
    lt: Point,
    rb: Point,
}

impl kdvtree::BoundingVolume<Point> for Bound {
    fn min_corner(&self) -> Point { self.lt }
    fn max_corner(&self) -> Point { self.rb }
}

fn get_bounding_volume(shape: &Segment) -> Bound {
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

#[derive(Default)]
struct PointsCutter {
    point_min: Option<Point>,
    point_max: Option<Point>,
}

impl<'s> kdvtree::GetCutPoint<Axis, Point> for &'s mut PointsCutter {
    fn cut_point<I>(&mut self, _cut_axis: &Axis, points: I) -> Option<Point> where I: Iterator<Item = Point> {
        self.point_min = None;
        self.point_max = None;
        let mut point_sum = Point { x: 0., y: 0., };
        let mut total = 0;
        for p in points {
            let pmin = self.point_min.get_or_insert(p);
            if p.x < pmin.x { pmin.x = p.x; }
            if p.y < pmin.y { pmin.y = p.y; }
            let pmax = self.point_max.get_or_insert(p);
            if p.x > pmax.x { pmax.x = p.x; }
            if p.y > pmax.y { pmax.y = p.y; }
            point_sum.x += p.x;
            point_sum.y += p.y;
            total += 1;
        }
        if total == 0 {
            None
        } else {
            Some(Point {
                x: point_sum.x / total as f64,
                y: point_sum.y / total as f64,
            })
        }
    }
}

fn cut_segment_fragment(shape: &Segment, fragment: &Bound, cut_axis: &Axis, cut_point: &Point) -> Result<Option<(Bound, Bound)>, ()> {
    match cut_axis {
        &Axis::X => if cut_point.x >= fragment.lt.x && cut_point.x <= fragment.rb.x {
            if fragment.rb.x - fragment.lt.x < KDTREE_CUT_LIMIT {
                Ok(None)
            } else {
                let factor = (cut_point.x - shape.src.x) / (shape.dst.x - shape.src.x);
                let y = shape.src.y + (factor * (shape.dst.y - shape.src.y));
                let left_point = if shape.src.x < shape.dst.x { shape.src } else { shape.dst };
                let left_bound = Bound {
                    lt: Point {
                        x: fragment.lt.x,
                        y: if left_point.y < y { fragment.lt.y } else { y },
                    },
                    rb: Point {
                        x: cut_point.x,
                        y: if left_point.y < y { y } else { fragment.rb.y },
                    }
                };
                let right_point = if shape.src.x < shape.dst.x { shape.dst } else { shape.src };
                let right_bound = Bound {
                    lt: Point {
                        x: cut_point.x,
                        y: if right_point.y < y { fragment.lt.y } else { y },
                    },
                    rb: Point {
                        x: fragment.rb.x,
                        y: if right_point.y < y { y } else { fragment.rb.y },
                    },
                };
                Ok(Some((left_bound, right_bound)))
            }
        } else {
            return Ok(None);
        },
        &Axis::Y => if cut_point.y >= fragment.lt.y && cut_point.y <= fragment.rb.y {
            if fragment.rb.y - fragment.lt.y < KDTREE_CUT_LIMIT {
                Ok(None)
            } else {
                let factor = (cut_point.y - shape.src.y) / (shape.dst.y - shape.src.y);
                let x = shape.src.x + (factor * (shape.dst.x - shape.src.x));
                let upper_point = if shape.src.y < shape.dst.y { shape.src } else { shape.dst };
                let upper_bound = Bound {
                    lt: Point {
                        x: if upper_point.x < x { fragment.lt.x } else { x },
                        y: fragment.lt.y,
                    },
                    rb: Point {
                        x: if upper_point.x < x { x } else { fragment.rb.x },
                        y: cut_point.y,
                    }
                };
                let lower_point = if shape.src.y < shape.dst.y { shape.dst } else { shape.src };
                let lower_bound = Bound {
                    lt: Point {
                        x: if lower_point.x < x { fragment.lt.x } else { x },
                        y: cut_point.y,
                        },
                    rb: Point {
                        x: if lower_point.x < x { x } else { fragment.rb.x },
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

fn bound_to_cut_point_dist(axis: &Axis, bounding_volume: &Bound, cut_point: &Point) -> f64 {
    match axis {
        &Axis::X => {
            let l = (bounding_volume.lt.x - cut_point.x).abs();
            let r = (bounding_volume.rb.x - cut_point.x).abs();
            if l < r { l } else { r }
        },
        &Axis::Y => {
            let t = (bounding_volume.lt.y - cut_point.y).abs();
            let b = (bounding_volume.rb.y - cut_point.y).abs();
            if t < b { t } else { b }
        },
    }
}

fn bound_to_bound_dist(bv_a: &Bound, bv_b: &Bound) -> f64 {
    fn dist(xa: f64, ya: f64, xb: f64, yb: f64) -> f64 {
        ((xb - xa) * (xb - xa) + (yb - ya) * (yb - ya)).sqrt()
    }
    let left = bv_b.rb.x < bv_a.lt.x;
    let right = bv_a.rb.x < bv_b.lt.x;
    let top = bv_a.rb.y < bv_b.lt.y;
    let bottom = bv_b.rb.y < bv_a.lt.y;
    if top && left {
        dist(bv_a.lt.x, bv_a.rb.y, bv_b.rb.x, bv_b.lt.y)
    } else if left && bottom {
        dist(bv_a.lt.x, bv_a.lt.y, bv_b.rb.x, bv_b.rb.y)
    } else if bottom && right {
        dist(bv_a.rb.x, bv_a.lt.y, bv_b.lt.x, bv_b.rb.y)
    } else if right && top {
        dist(bv_a.rb.x, bv_a.rb.y, bv_b.lt.x, bv_b.lt.y)
    } else if left {
        bv_a.lt.x - bv_b.rb.x
    } else if right {
        bv_b.lt.x - bv_a.rb.x
    } else if bottom {
        bv_a.lt.y - bv_b.rb.y
    } else if top {
        bv_b.lt.y - bv_a.rb.y
    } else {
        0.
    }
}

struct VisualCutter {
    cuts: Vec<(Segment, Axis)>,
    base_cutter: PointsCutter,
}

impl VisualCutter {
    fn new() -> VisualCutter {
        VisualCutter {
            cuts: Vec::new(),
            base_cutter: Default::default(),
        }
    }
}

impl<'s> kdvtree::GetCutPoint<Axis, Point> for &'s mut VisualCutter {
    fn cut_point<I>(&mut self, cut_axis: &Axis, points: I) -> Option<Point> where I: Iterator<Item = Point> {
        if let Some(point_mid) = kdvtree::GetCutPoint::cut_point(&mut &mut self.base_cutter, cut_axis, points) {
            if let (Some(pmin), Some(pmax)) = (self.base_cutter.point_min, self.base_cutter.point_max) {
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
        } else {
            None
        }
    }
}
