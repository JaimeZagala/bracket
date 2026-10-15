//! Throwaway iced 0.14 prototype used to compare UI frameworks for Bracket.
//!
//! Opens an optional UTF-8 file into a rope off the UI thread and draws only the visible lines on
//! a canvas, with a line-number gutter and keyboard/mouse-wheel scrolling. Not meant to be merged.

// Release builds are GUI applications without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use iced::keyboard::{self, Key, key::Named};
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Text};
use iced::widget::text::{Alignment, LineHeight};
use iced::widget::{center, text};
use iced::{
    Color, Element, Fill, Font, Pixels, Point, Rectangle, Renderer, Size, Subscription, Task,
    Theme, mouse, window,
};
use ropey::Rope;

const FONT: Font = Font::with_name("Consolas");
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 18.0;
/// Consolas advance width is 1126/2048 em; good enough to size the gutter and clip long lines.
const CHAR_WIDTH: f32 = FONT_SIZE * 1126.0 / 2048.0;
const TAB_WIDTH: usize = 4;
const WHEEL_LINES: f32 = 3.0;
const PADDING: f32 = 8.0;

/// Startup probe compatible with `scripts/measure.ps1`.
#[derive(Debug, Clone)]
struct Probe {
    output: PathBuf,
    main_wall: SystemTime,
    main_instant: Instant,
}

impl Probe {
    fn finish(&self) -> ! {
        let report = format!(
            "main_unix_us={}\nframe_unix_us={}\nmain_to_frame_ms={:.1}\n",
            unix_micros(self.main_wall),
            unix_micros(SystemTime::now()),
            self.main_instant.elapsed().as_secs_f64() * 1000.0
        );
        // Best effort: a failed write simply shows up as a missing measurement.
        let _ = std::fs::write(&self.output, report);
        std::process::exit(0);
    }
}

fn unix_micros(t: SystemTime) -> u128 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0)
}

fn main() -> iced::Result {
    // Taken first so the startup measurement covers as much as possible.
    let main_wall = SystemTime::now();
    let main_instant = Instant::now();
    let probe = std::env::var_os("BRACKET_MEASURE_STARTUP").map(|output| Probe {
        output: PathBuf::from(output),
        main_wall,
        main_instant,
    });
    // The two measurement modes are not combined: the startup probe takes precedence.
    let bench = match probe {
        Some(_) => None,
        None => std::env::var_os("BRACKET_MEASURE_SCROLL").map(ScrollBench::new),
    };
    let maximized = std::env::var_os("BRACKET_MEASURE_MAXIMIZED").is_some_and(|v| v == "1");
    let path = std::env::args_os().nth(1).map(PathBuf::from);

    iced::application(
        move || App::boot(path.clone(), probe.clone(), bench.clone()),
        App::update,
        App::view,
    )
    .title("Bracket iced prototype")
    .window(window::Settings {
        size: Size::new(1000.0, 700.0),
        maximized,
        ..Default::default()
    })
    .subscription(App::subscription)
    .theme(Theme::Dark)
    .run()
}

/// Scroll-cost measurement: one page per frame for `SCROLL_FRAMES` presented frames.
#[derive(Debug, Clone)]
struct ScrollBench {
    output: PathBuf,
    started: bool,
    frames: u32,
    first_frame: Option<Instant>,
}

const SCROLL_FRAMES: u32 = 300;

impl ScrollBench {
    fn new(output: std::ffi::OsString) -> Self {
        Self {
            output: PathBuf::from(output),
            started: false,
            frames: 0,
            first_frame: None,
        }
    }

    fn finish(&self, last_frame: Instant) -> ! {
        let elapsed = self
            .first_frame
            .map_or(0.0, |first| (last_frame - first).as_secs_f64());
        let report = format!(
            "frames={}\nscroll_ms={:.1}\n",
            self.frames,
            elapsed * 1000.0
        );
        // Best effort: a failed write simply shows up as a missing measurement.
        let _ = std::fs::write(&self.output, report);
        std::process::exit(0);
    }
}

#[derive(Debug, Clone)]
enum Message {
    Loaded(Result<Rope, String>),
    /// Scroll by `delta` lines; `visible` is the number of lines that fit in the canvas.
    Scroll {
        delta: i64,
        visible: usize,
    },
    /// The canvas reports how many whole rows fit when that number changes.
    Viewport(usize),
    Frame(Instant),
}

struct App {
    probe: Option<Probe>,
    bench: Option<ScrollBench>,
    /// Whole rows that fit in the canvas, as last reported by it (0 until the first redraw).
    visible: usize,
    /// `None` while the file is loading.
    content: Option<Result<Rope, String>>,
    /// First visible line.
    scroll: usize,
    /// When the final content became part of the view (for the startup probe).
    ready_at: Option<Instant>,
}

impl App {
    fn boot(
        path: Option<PathBuf>,
        probe: Option<Probe>,
        bench: Option<ScrollBench>,
    ) -> (Self, Task<Message>) {
        let mut app = Self {
            probe,
            bench,
            visible: 0,
            content: None,
            scroll: 0,
            ready_at: None,
        };
        let task = match path {
            // The default executor (`thread-pool` feature) runs this future off the UI thread.
            Some(path) => Task::perform(async move { load(path) }, Message::Loaded),
            None => {
                app.content = Some(Ok(Rope::new()));
                app.ready_at = Some(Instant::now());
                Task::none()
            }
        };
        (app, task)
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Loaded(result) => {
                self.content = Some(result);
                self.ready_at = Some(Instant::now());
            }
            Message::Scroll { delta, visible } => {
                if let Some(Ok(rope)) = &self.content {
                    let max = rope.len_lines().saturating_sub(visible) as i64;
                    self.scroll = (self.scroll as i64 + delta).clamp(0, max) as usize;
                }
            }
            Message::Viewport(visible) => self.visible = visible,
            Message::Frame(at) => {
                // `at` is taken right before the redraw that emitted it, and the message is only
                // handled after that redraw has been presented. A frame that started after the
                // content was ready therefore drew the content and is already on screen.
                let content_presented = self.ready_at.is_some_and(|ready| at > ready);
                if let Some(probe) = &self.probe
                    && content_presented
                {
                    probe.finish();
                }
                if content_presented && self.visible > 0 {
                    self.scroll_bench_frame();
                }
            }
        }
    }

    /// Called for every presented frame once the content is on screen. Each call scrolls one page;
    /// handling this message makes iced rebuild the view and redraw, so the next frame event
    /// belongs to a frame that drew the new lines.
    fn scroll_bench_frame(&mut self) {
        let Some(bench) = &mut self.bench else { return };
        if bench.started {
            bench.frames += 1;
            let now = Instant::now();
            if bench.frames == 1 {
                bench.first_frame = Some(now);
            }
            if bench.frames == SCROLL_FRAMES {
                bench.finish(now);
            }
        } else {
            bench.started = true;
        }
        let lines = match &self.content {
            Some(Ok(rope)) => rope.len_lines(),
            _ => 1,
        };
        let max = lines.saturating_sub(self.visible);
        let next = self.scroll + self.visible;
        self.scroll = if next > max { 0 } else { next };
    }

    fn view(&self) -> Element<'_, Message> {
        match &self.content {
            None => center(text("Loading…")).into(),
            Some(Err(error)) => center(text(error.as_str())).into(),
            Some(Ok(rope)) => Canvas::new(TextView {
                rope,
                scroll: self.scroll,
                known_visible: self.visible,
            })
            .width(Fill)
            .height(Fill)
            .into(),
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        // Every handled message triggers a redraw, so this keeps frames coming while measuring.
        if self.probe.is_some() || self.bench.is_some() {
            window::frames().map(Message::Frame)
        } else {
            Subscription::none()
        }
    }
}

fn load(path: PathBuf) -> Result<Rope, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = std::str::from_utf8(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Rope::from_str(text))
}

/// Canvas program that draws the visible lines of a rope.
struct TextView<'a> {
    rope: &'a Rope,
    scroll: usize,
    /// Row count the app already knows about.
    known_visible: usize,
}

impl TextView<'_> {
    fn gutter_width(&self) -> f32 {
        let digits = self.rope.len_lines().to_string().len().max(3);
        digits as f32 * CHAR_WIDTH + 2.0 * PADDING
    }
}

impl canvas::Program<Message> for TextView<'_> {
    /// Fractional wheel lines not applied yet (touchpads send small pixel deltas).
    type State = f32;

    fn update(
        &self,
        pending: &mut f32,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let visible = (bounds.height / LINE_HEIGHT).floor().max(1.0) as usize;
        let page = visible.saturating_sub(1).max(1) as i64;
        let delta = match event {
            // Every redraw reaches the canvas first; report the row count when it changes.
            canvas::Event::Window(window::Event::RedrawRequested(_)) => {
                return (visible != self.known_visible)
                    .then(|| canvas::Action::publish(Message::Viewport(visible)));
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.is_over(bounds) =>
            {
                *pending -= match *delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * WHEEL_LINES,
                    mouse::ScrollDelta::Pixels { y, .. } => y / LINE_HEIGHT,
                };
                let whole = pending.trunc();
                *pending -= whole;
                whole as i64
            }
            canvas::Event::Keyboard(keyboard::Event::KeyPressed {
                key: Key::Named(named),
                ..
            }) => match named {
                Named::ArrowUp => -1,
                Named::ArrowDown => 1,
                Named::PageUp => -page,
                Named::PageDown => page,
                _ => return None,
            },
            _ => return None,
        };
        (delta != 0)
            .then(|| canvas::Action::publish(Message::Scroll { delta, visible }).and_capture())
    }

    fn draw(
        &self,
        _state: &f32,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let gutter = self.gutter_width();
        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(gutter, bounds.height),
            Color::from_rgb8(30, 30, 30),
        );

        let rows = (bounds.height / LINE_HEIGHT).ceil() as usize;
        let max_columns = ((bounds.width - gutter) / CHAR_WIDTH).ceil().max(0.0) as usize;
        let last = (self.scroll + rows).min(self.rope.len_lines());
        for (row, index) in (self.scroll..last).enumerate() {
            let y = row as f32 * LINE_HEIGHT;
            let base = Text {
                size: Pixels(FONT_SIZE),
                line_height: LineHeight::Absolute(Pixels(LINE_HEIGHT)),
                font: FONT,
                ..Text::default()
            };
            frame.fill_text(Text {
                content: (index + 1).to_string(),
                position: Point::new(gutter - PADDING, y),
                color: Color::from_rgb8(120, 120, 120),
                align_x: Alignment::Right,
                ..base.clone()
            });
            frame.fill_text(Text {
                content: visible_text(self.rope.line(index), max_columns),
                position: Point::new(gutter + PADDING, y),
                color: Color::from_rgb8(220, 220, 220),
                ..base
            });
        }
        vec![frame.into_geometry()]
    }
}

/// Returns the first `max_columns` columns of a line, without its line break and with tabs
/// expanded to the next multiple of `TAB_WIDTH`.
fn visible_text(line: ropey::RopeSlice<'_>, max_columns: usize) -> String {
    let mut out = String::new();
    let mut column = 0;
    for ch in line.chars() {
        if column >= max_columns || ch == '\n' || ch == '\r' {
            break;
        }
        if ch == '\t' {
            let spaces = TAB_WIDTH - column % TAB_WIDTH;
            out.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            out.push(ch);
            column += 1;
        }
    }
    out
}
