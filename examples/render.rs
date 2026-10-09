//! Renders the demo interface to PNG without a window: no Dock icon, no app
//! switcher entry, no focus taken. egui's test harness runs the real app
//! frames and its wgpu renderer draws them into an offscreen texture (Metal
//! on macOS, Vulkan or DX12 elsewhere).
//!
//! It takes the states `--demo-shot` takes:
//!
//!   cargo run --example render --features render -- \
//!     --page album:alb0 --show dark,queue --size 1280x800 --out album.png
//!
//! Cover art is downloaded like in the demo, so the capture waits until the
//! artwork has loaded (or `--settle-ms` passes). With `--frames N` it writes
//! N numbered pictures `--step-ms` apart for inspecting motion; a `--click`
//! or `--drag` then presses on the first of them.
//!
//! `packaging/render-shots.sh` renders a list of states in one go.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use clap::Parser;
use egui_kittest::Harness;
use spotifast::app::App;
use spotifast::demo::script::{self, Pointer};

/// Render the Spotifast demo interface headlessly.
#[derive(Debug, Parser)]
struct Args {
    /// Page to open, as `--demo-page` takes it: `home`, `album:alb0`,
    /// `playlist:pl1`, `artist:art0`, `settings`, ...
    #[arg(long)]
    page: Option<String>,

    /// Extra surfaces, as `--demo-show` takes them: `dark`, `light`,
    /// `queue`, `friends`, `lyrics`, `devices`, ... comma-separated.
    #[arg(long)]
    show: Option<String>,

    /// Light or dark, added to `--show`, so a batch can render each state
    /// in both themes.
    #[arg(long, value_enum)]
    theme: Option<Theme>,

    /// Interface language, as `--demo-language` takes it.
    #[arg(long, value_enum)]
    language: Option<spotifast::i18n::Locale>,

    /// Window size in points.
    #[arg(long, value_name = "WIDTHxHEIGHT", default_value = "1280x800", value_parser = script::parse_size)]
    size: [f32; 2],

    /// Pixels per point: 2 matches a Retina display.
    #[arg(long, default_value_t = 1.0)]
    scale: f32,

    /// Click once at `X,Y` in points, as `--demo-click` does.
    #[arg(long, value_name = "X,Y", value_parser = script::parse_point, conflicts_with = "drag")]
    click: Option<egui::Pos2>,

    /// Press at the first point and hold at the second, as `--demo-drag`.
    #[arg(long, value_name = "X,Y:X,Y", value_parser = script::parse_drag)]
    drag: Option<[egui::Pos2; 2]>,

    /// The PNG to write. With `--frames` above 1, `-000`, `-001`, ... go
    /// before the extension.
    #[arg(long)]
    out: PathBuf,

    /// Pictures to write, one per frame.
    #[arg(long, default_value_t = 1)]
    frames: u32,

    /// Time between frames, in egui's clock and on the wall clock.
    #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u64).range(1..))]
    step_ms: u64,

    /// The longest to wait for artwork and pending work before capturing.
    #[arg(long, default_value_t = 30_000)]
    settle_ms: u64,

    /// Where the demo keeps its files. Each run starts from fresh settings
    /// and state in a folder of its own; the art cache under `cache/` is
    /// shared so later runs need not download it again.
    #[arg(long)]
    data: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum Theme {
    Dark,
    Light,
}

impl Args {
    /// `--show` with `--theme` added, in the form `apply_flags` reads.
    fn surfaces(&self) -> Option<String> {
        let theme = self.theme.map(|theme| match theme {
            Theme::Dark => "dark",
            Theme::Light => "light",
        });
        let surfaces: Vec<&str> = self.show.as_deref().into_iter().chain(theme).collect();
        (!surfaces.is_empty()).then(|| surfaces.join(","))
    }
}

/// What the harness hands each frame: the app, made on the first frame so
/// it attaches to the harness's own context.
struct Scene {
    app: Option<App>,
    dirs: spotifast::paths::AppDirs,
    page: Option<String>,
    show: Option<String>,
    language: Option<spotifast::i18n::Locale>,
}

impl Scene {
    fn frame(ui: &mut egui::Ui, scene: &mut Self) {
        // The app's fonts and style take effect from the frame after it
        // attaches, as eframe's app creator runs before the first frame.
        if scene.app.is_none() {
            scene.app = Some(scene.create(ui.ctx()));
            ui.ctx().request_repaint();
            return;
        }
        let Some(app) = scene.app.as_mut() else {
            return;
        };
        // The harness hands over a framed, inset Ui; the app gets the whole
        // viewport, as eframe gives it.
        let ctx = ui.ctx().clone();
        let mut root = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("render-root"),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::background())
                .max_rect(ctx.viewport_rect()),
        );
        app.background_frame(&ctx);
        app.frame_ui(&mut root);
    }

    fn create(&self, ctx: &egui::Context) -> App {
        let mut app = spotifast::demo::headless_app(ctx, self.dirs.clone());
        // The same order the binary's demo launch takes.
        app.open_friends_by_default();
        spotifast::demo::apply_flags(&mut app, self.page.as_deref(), self.show.as_deref());
        if let Some(locale) = self.language {
            script::set_language(&mut app, locale);
        }
        // The notch card is a native panel of its own.
        app.settings.mac_notch_widget = false;
        app
    }
}

/// Frames that must pass with nothing loading before the picture is taken,
/// so decoded art has reached a texture and accents have been worked out.
const QUIET_FRAMES: u32 = 12;
/// Frames always run first, so the first pages and requests are under way.
const WARM_UP_FRAMES: u32 = 30;

fn main() -> Result<()> {
    let args = Args::parse();
    fastframe_log::Logging::new("spotifast", env!("CARGO_PKG_VERSION"))
        .filter("warn")
        .init()?;
    spotifast::emoji::install(true);

    let base = args.data.clone().unwrap_or_else(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("render-data")
    });
    let run = base.join(format!("run-{}", std::process::id()));
    let dirs = spotifast::paths::AppDirs {
        config: run.join("config"),
        state: run.join("state"),
        cache: base.join("cache"),
    };
    dirs.ensure().context("creating the demo folders")?;
    // Off the main thread, AppKit calls the interface makes for a real
    // window (a title-bar double-click, minimizing) find no main-thread
    // marker and do nothing, so nothing here can reach the window server.
    let result = std::thread::Builder::new()
        .name("render".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || render(&args, dirs))
        .context("starting the render thread")?
        .join()
        .unwrap_or_else(|_| Err(anyhow::anyhow!("the render thread panicked")));
    let _ = std::fs::remove_dir_all(&run);
    result
}

fn render(args: &Args, dirs: spotifast::paths::AppDirs) -> Result<()> {
    let step = Duration::from_millis(args.step_ms);
    let scene = Scene {
        app: None,
        dirs,
        page: args.page.clone(),
        show: args.surfaces(),
        language: args.language,
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(args.size[0], args.size[1]))
        .with_pixels_per_point(args.scale)
        .with_os(egui::os::OperatingSystem::from_target_os())
        .with_step_dt(step.as_secs_f32())
        .with_wait_for_pending_images(false)
        .wgpu()
        .build_ui_state(Scene::frame, scene);

    let deadline = Instant::now() + Duration::from_millis(args.settle_ms);
    settle(&mut harness, step, deadline);

    let mut pointer = Pointer::from_flags(args.drag, args.click);
    if let Some(pointer) = pointer.as_mut() {
        // Rest on the starting point, so hover states are drawn, up to the
        // frame before the press.
        for _ in 0..Pointer::REST {
            frame(&mut harness, Some(pointer), step);
        }
    }

    if args.frames <= 1 {
        if let Some(pointer) = pointer.as_mut() {
            while !pointer.done() {
                frame(&mut harness, Some(pointer), step);
            }
            settle(&mut harness, step, deadline.max(Instant::now() + step * 60));
        }
        return save(&mut harness, &args.out);
    }
    for index in 0..args.frames {
        frame(&mut harness, pointer.as_mut(), step);
        save(&mut harness, &numbered(&args.out, index))?;
    }
    Ok(())
}

/// One frame, `step` after the last on both clocks, with the scripted
/// pointer's events for it.
fn frame(harness: &mut Harness<'_, Scene>, pointer: Option<&mut Pointer>, step: Duration) {
    if let Some(pointer) = pointer.filter(|pointer| !pointer.done()) {
        harness.input_mut().events.extend(pointer.events());
    }
    harness.step();
    std::thread::sleep(step);
}

/// Runs frames until artwork and decoding have been quiet for a while, or
/// until `deadline`.
fn settle(harness: &mut Harness<'_, Scene>, step: Duration, deadline: Instant) {
    let mut quiet = 0;
    let mut frames = 0;
    while Instant::now() < deadline {
        frame(harness, None, step);
        frames += 1;
        quiet = if harness.ctx.has_pending_images() {
            0
        } else {
            quiet + 1
        };
        if frames >= WARM_UP_FRAMES && quiet >= QUIET_FRAMES {
            return;
        }
    }
    eprintln!("still loading after the settle time; capturing anyway");
}

fn numbered(path: &Path, index: u32) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let extension = path
        .extension()
        .map_or_else(|| "png".into(), |ext| ext.to_string_lossy());
    path.with_file_name(format!("{stem}-{index:03}.{extension}"))
}

fn save(harness: &mut Harness<'_, Scene>, path: &Path) -> Result<()> {
    let image = harness
        .render()
        .map_err(|error| anyhow::anyhow!("rendering failed: {error}"))?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    image
        .save(path)
        .with_context(|| format!("writing {}", path.display()))?;
    println!(
        "wrote {}x{} to {}",
        image.width(),
        image.height(),
        path.display()
    );
    Ok(())
}
