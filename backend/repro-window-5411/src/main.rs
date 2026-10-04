#[cfg(not(windows))]
fn main() {
    eprintln!("This reproducer requires Windows.");
    std::process::exit(2);
}

#[cfg(windows)]
fn main() {
    repro::run();
}

#[cfg(windows)]
mod repro {
    use tao::{
        dpi::{LogicalSize, PhysicalPosition, PhysicalSize, Size},
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget},
        platform::windows::{WindowBuilderExtWindows, WindowExtWindows},
        window::{Window, WindowBuilder},
    };

    const ROUNDS: u32 = 5;
    const INITIAL_RESTORE_SIZE: PhysicalSize<u32> = PhysicalSize::new(1266, 943);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Sizing {
        Immediate,
        AfterEvents,
        Builder,
    }

    #[derive(Debug, Clone, Copy)]
    struct Options {
        decorated: bool,
        shadow: bool,
        transparent: bool,
        visible_on_create: bool,
        sizing: Sizing,
    }

    fn options() -> Options {
        let mut options = Options {
            decorated: false,
            shadow: true,
            transparent: true,
            visible_on_create: false,
            sizing: Sizing::Immediate,
        };
        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--decorated" => options.decorated = true,
                "--no-shadow" => options.shadow = false,
                "--opaque" => options.transparent = false,
                "--visible" => options.visible_on_create = true,
                "--defer-restore" => options.sizing = Sizing::AfterEvents,
                "--builder-size" => options.sizing = Sizing::Builder,
                _ => {
                    eprintln!("Unknown argument: {arg}");
                    eprintln!(
                        "Options: --decorated --no-shadow --opaque --visible \
                         --defer-restore --builder-size"
                    );
                    std::process::exit(2);
                }
            }
        }
        options
    }

    fn snapshot(window: &Window, round: u32, stage: &str, requested: PhysicalSize<u32>) {
        let inner = window.inner_size();
        let outer = window.outer_size();
        let non_client = (
            i64::from(outer.width) - i64::from(inner.width),
            i64::from(outer.height) - i64::from(inner.height),
        );
        println!(
            "round={round} stage={stage} hwnd={:?} requested={requested:?} inner={inner:?} \
             outer={outer:?} non_client={non_client:?} position={:?} scale={} visible={} \
             decorated={} shadow={}",
            window.hwnd(),
            window.outer_position(),
            window.scale_factor(),
            window.is_visible(),
            window.is_decorated(),
            window.has_undecorated_shadow(),
        );
    }

    fn create_window(
        target: &EventLoopWindowTarget<()>,
        options: Options,
        round: u32,
        requested: PhysicalSize<u32>,
    ) -> Window {
        let initial_size = match options.sizing {
            Sizing::Builder => Size::Physical(requested),
            _ => Size::Logical(LogicalSize::new(800.0, 800.0)),
        };
        let window = WindowBuilder::new()
            .with_title("Tao-only window geometry repro #5411")
            .with_inner_size(initial_size)
            .with_min_inner_size(LogicalSize::new(400.0, 600.0))
            .with_position(PhysicalPosition::new(0, 0))
            .with_decorations(options.decorated)
            .with_undecorated_shadow(options.shadow)
            .with_transparent(options.transparent)
            .with_visible(options.visible_on_create)
            .build(target)
            .expect("create Tao window");

        let expected_initial = initial_size.to_physical::<u32>(window.scale_factor());
        snapshot(&window, round, "created", expected_initial);
        println!("round={round} monitor={:?}", window.current_monitor());

        window.set_outer_position(PhysicalPosition::new(100, 100));
        if options.sizing != Sizing::AfterEvents {
            restore_and_show(&window, options.sizing, round, requested);
        }
        window
    }

    fn restore_and_show(window: &Window, sizing: Sizing, round: u32, requested: PhysicalSize<u32>) {
        if sizing == Sizing::Builder {
            snapshot(window, round, "before_show_no_setter", requested);
        } else {
            snapshot(window, round, "before_restore", requested);
            window.set_inner_size(requested);
            snapshot(window, round, "restore_submitted", requested);
        }
        window.set_visible(true);
        snapshot(window, round, "show_submitted", requested);
    }

    pub fn run() {
        let options = options();
        println!("tao=0.37.1 options={options:?} rounds={ROUNDS}");
        println!(
            "All sizes and positions are physical pixels; builder size is 800x800 logical \
             unless --builder-size supplies the physical target directly."
        );

        let event_loop = EventLoop::new();
        let proxy = event_loop.create_proxy();
        let mut round = 1;
        let mut requested = INITIAL_RESTORE_SIZE;
        let mut window = Some(create_window(&event_loop, options, round, requested));
        let mut closing_id = None;
        let mut restore_mismatches = 0;
        let mut awaiting_restore = options.sizing == Sizing::AfterEvents;

        event_loop.run(move |event, target, control_flow| {
            *control_flow = ControlFlow::Wait;
            match event {
                Event::WindowEvent {
                    window_id, event, ..
                } => {
                    if window
                        .as_ref()
                        .is_some_and(|window| window.id() == window_id)
                    {
                        match event {
                            WindowEvent::Resized(_)
                            | WindowEvent::ScaleFactorChanged { .. }
                            | WindowEvent::Focused(_) => {
                                println!("round={round} event={event:?}");
                            }
                            _ => {}
                        }
                    }
                    if matches!(event, WindowEvent::Destroyed) && closing_id == Some(window_id) {
                        println!("round={round} event=Destroyed");
                        closing_id = None;
                        if round == ROUNDS {
                            println!(
                                "SUMMARY rounds={ROUNDS} restore_mismatches={restore_mismatches} \
                                 initial={INITIAL_RESTORE_SIZE:?} final={requested:?}"
                            );
                            *control_flow = ControlFlow::Exit;
                        } else {
                            round += 1;
                            window = Some(create_window(target, options, round, requested));
                            awaiting_restore = options.sizing == Sizing::AfterEvents;
                        }
                    }
                }
                Event::MainEventsCleared => {
                    if awaiting_restore {
                        let current = window.as_ref().expect("a window is awaiting restoration");
                        let expected_initial = LogicalSize::new(800.0, 800.0)
                            .to_physical::<u32>(current.scale_factor());
                        snapshot(current, round, "created_after_events", expected_initial);
                        restore_and_show(current, options.sizing, round, requested);
                        awaiting_restore = false;
                        // Wake another event batch for measurement without changing geometry.
                        proxy.send_event(()).expect("schedule final measurement");
                    } else if let Some(current) = window.take() {
                        snapshot(&current, round, "before_destroy", requested);
                        let actual = current.inner_size();
                        let delta = (
                            i64::from(actual.width) - i64::from(requested.width),
                            i64::from(actual.height) - i64::from(requested.height),
                        );
                        let mismatch = actual != requested;
                        restore_mismatches += u32::from(mismatch);
                        println!("round={round} restore_mismatch={mismatch} delta={delta:?}");
                        requested = actual;
                        closing_id = Some(current.id());
                        drop(current);
                    }
                }
                Event::LoopDestroyed if restore_mismatches > 0 => std::process::exit(1),
                _ => {}
            }
        });
    }
}
