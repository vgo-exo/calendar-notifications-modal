//! Calendar Notifications Modal daemon.
//!
//! A single long-running process that polls calendar backends on a worker
//! thread and, on the GTK main thread, evaluates which events are due and
//! shows/updates/hides the reminder modal.

mod config;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Local, Utc};
use cnm_backends::{GoogleBackend, IcsBackend, MsGraphBackend};
use cnm_core::backend::CalendarBackend;
use cnm_core::model::{CalendarEvent, ReminderState};
use cnm_engine::{compute_due, resolve_snooze, DueEvent, EngineConfig};
use cnm_store::Store;
use cnm_ui::{ReminderRow, ReminderWindow, UiAction};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::Application;

use config::Config;

const APP_ID: &str = "com.github.calendar_notifications_modal";

/// Shared snapshot of events fetched by the worker thread.
type EventSnapshot = Arc<Mutex<Vec<CalendarEvent>>>;

/// Everything the GTK main thread needs to evaluate and render reminders.
struct AppCtx {
    store: Store,
    engine_cfg: EngineConfig,
    events: EventSnapshot,
    window: RefCell<Option<Rc<ReminderWindow>>>,
    _hold: RefCell<Option<gtk4::gio::ApplicationHoldGuard>>,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cfg_path = match config::default_config_path() {
        Some(p) => p,
        None => {
            eprintln!("could not determine config directory");
            std::process::exit(1);
        }
    };
    let cfg = match config::load_or_init(&cfg_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to load config {}: {e}", cfg_path.display());
            std::process::exit(1);
        }
    };
    tracing::info!(
        "loaded config from {} with {} backend(s)",
        cfg_path.display(),
        cfg.backends.len()
    );

    // CLI: a one-time `--login <backend-id>` device-code sign-in for a Microsoft
    // Graph backend. Runs to completion and exits before the GTK loop starts.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--login") {
        let Some(id) = args.get(pos + 1) else {
            eprintln!("--login requires a backend id, e.g. --login outlook");
            std::process::exit(2);
        };
        match run_login(&cfg, id) {
            Ok(()) => return,
            Err(e) => {
                eprintln!("login failed: {e}");
                std::process::exit(1);
            }
        }
    }

    // Validate we can open the state database before entering the UI loop.
    if let Err(e) = Store::open_default() {
        eprintln!("failed to open state database: {e}");
        std::process::exit(1);
    }

    // Snapshot shared with the polling worker thread.
    let events: EventSnapshot = Arc::new(Mutex::new(Vec::new()));
    spawn_poller(cfg.clone(), events.clone());

    let engine_cfg = EngineConfig {
        global_default_reminder: cfg.global_reminder(),
        before_start_presets: cfg.snooze.before_start.clone(),
        after_now_presets: cfg.snooze.after_now.clone(),
    };

    let refresh = StdDuration::from_secs(cfg.refresh_interval_secs.max(5));

    let app = Application::builder().application_id(APP_ID).build();

    app.connect_activate(move |app| {
        tracing::info!("activate: building reminder window");
        let ctx = Rc::new(AppCtx {
            store: Store::open_default().expect("reopen store"),
            engine_cfg: engine_cfg.clone(),
            events: events.clone(),
            window: RefCell::new(None),
            _hold: RefCell::new(Some(app.hold())),
        });

        // Callback bridges UI events back to the controller without creating a
        // strong reference cycle (window -> callback -> ctx -> window).
        let weak: Weak<AppCtx> = Rc::downgrade(&ctx);
        let callback: Rc<dyn Fn(UiAction)> = Rc::new(move |action| {
            if let Some(ctx) = weak.upgrade() {
                handle_action(&ctx, action);
            }
        });

        let window = ReminderWindow::new(app, callback);
        *ctx.window.borrow_mut() = Some(window);

        // Periodic re-evaluation so reminders appear as their trigger time
        // arrives and snoozes expire, independent of the polling cadence. This
        // timer owns a strong reference, keeping the controller (and window)
        // alive for the lifetime of the daemon.
        let tick_ctx = ctx.clone();
        glib::timeout_add_local(refresh, move || {
            recompute(&tick_ctx);
            glib::ControlFlow::Continue
        });

        // First evaluation shortly after startup (give the poller a moment).
        let weak_first: Weak<AppCtx> = Rc::downgrade(&ctx);
        glib::timeout_add_local_once(StdDuration::from_secs(2), move || {
            if let Some(ctx) = weak_first.upgrade() {
                recompute(&ctx);
            }
        });
    });

    // Run without passing CLI args to GTK.
    let empty: [&str; 0] = [];
    app.run_with_args(&empty);
}

/// Build the configured backends (worker-thread side).
fn build_backends(cfg: &Config) -> Vec<Box<dyn CalendarBackend>> {
    let mut backends: Vec<Box<dyn CalendarBackend>> = Vec::new();
    for b in &cfg.backends {
        match b.kind.as_str() {
            "ics" => {
                if let Some(url) = &b.url {
                    backends.push(Box::new(IcsBackend::from_url(b.id.clone(), url.clone())));
                } else if let Some(file) = &b.file {
                    backends.push(Box::new(IcsBackend::from_file(b.id.clone(), file.clone())));
                } else {
                    tracing::warn!("ics backend '{}' has neither `url` nor `file`", b.id);
                }
            }
            "msgraph" => {
                let Some(client_id) = b.client_id.clone() else {
                    tracing::warn!("msgraph backend '{}' is missing `client_id`", b.id);
                    continue;
                };
                let tenant = b.tenant.clone().unwrap_or_else(|| "common".to_string());
                let Some(cache_path) = msgraph_cache_path(&b.id) else {
                    tracing::warn!(
                        "msgraph backend '{}': could not determine token cache path",
                        b.id
                    );
                    continue;
                };
                backends.push(Box::new(MsGraphBackend::new(
                    b.id.clone(),
                    tenant,
                    client_id,
                    cache_path,
                )));
            }
            "google" => {
                let (Some(client_id), Some(client_secret)) =
                    (b.client_id.clone(), b.client_secret.clone())
                else {
                    tracing::warn!(
                        "google backend '{}' needs both `client_id` and `client_secret`",
                        b.id
                    );
                    continue;
                };
                let calendar_id = b
                    .calendar_id
                    .clone()
                    .unwrap_or_else(|| "primary".to_string());
                let Some(cache_path) = cnm_backends::auth::cache_path("google", &b.id) else {
                    tracing::warn!(
                        "google backend '{}': could not determine token cache path",
                        b.id
                    );
                    continue;
                };
                backends.push(Box::new(GoogleBackend::new(
                    b.id.clone(),
                    client_id,
                    client_secret,
                    calendar_id,
                    cache_path,
                )));
            }
            other => {
                tracing::warn!("backend type '{other}' (id '{}') is not yet implemented", b.id);
            }
        }
    }
    backends
}

/// Per-backend Microsoft Graph token cache path under the user's data dir.
fn msgraph_cache_path(id: &str) -> Option<PathBuf> {
    cnm_backends::auth::default_cache_path(id)
}

/// Print CLI usage.
fn print_usage() {
    println!(
        "calendar-notifications-modal\n\n\
         USAGE:\n  \
         calendar-notifications-modal            Run the reminder daemon\n  \
         calendar-notifications-modal --login ID Sign in a Microsoft Graph or Google backend (device code)\n  \
         calendar-notifications-modal --help     Show this help\n"
    );
}

/// Run the device-code sign-in flow for the backend with id `id`, dispatching on
/// its configured kind (`msgraph` or `google`).
fn run_login(cfg: &Config, id: &str) -> anyhow::Result<()> {
    let backend_cfg = cfg
        .backends
        .iter()
        .find(|b| b.id == id)
        .ok_or_else(|| anyhow::anyhow!("no backend with id '{id}' in config"))?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    match backend_cfg.kind.as_str() {
        "msgraph" => {
            let client_id = backend_cfg
                .client_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("msgraph backend '{id}' is missing `client_id`"))?;
            let tenant = backend_cfg
                .tenant
                .clone()
                .unwrap_or_else(|| "common".to_string());
            let cache_path = msgraph_cache_path(id)
                .ok_or_else(|| anyhow::anyhow!("could not determine token cache path"))?;
            let backend = MsGraphBackend::new(id.to_string(), tenant, client_id, cache_path);

            rt.block_on(async move {
                let prompt = backend.oauth().begin_device_code().await?;
                match &prompt.message {
                    Some(msg) => println!("\n{msg}\n"),
                    None => println!(
                        "\nTo sign in, open {} and enter the code: {}\n",
                        prompt.verification_uri, prompt.user_code
                    ),
                }
                println!("Waiting for you to complete sign-in in the browser...");
                let cache = backend.oauth().poll_for_token(&prompt).await?;
                cache.save(backend.cache_path())?;
                println!(
                    "Login successful. Tokens saved to {}",
                    backend.cache_path().display()
                );
                Ok::<(), anyhow::Error>(())
            })
        }
        "google" => {
            let client_id = backend_cfg
                .client_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("google backend '{id}' is missing `client_id`"))?;
            let client_secret = backend_cfg.client_secret.clone().ok_or_else(|| {
                anyhow::anyhow!("google backend '{id}' is missing `client_secret`")
            })?;
            let calendar_id = backend_cfg
                .calendar_id
                .clone()
                .unwrap_or_else(|| "primary".to_string());
            let cache_path = cnm_backends::auth::cache_path("google", id)
                .ok_or_else(|| anyhow::anyhow!("could not determine token cache path"))?;
            let backend =
                GoogleBackend::new(id.to_string(), client_id, client_secret, calendar_id, cache_path);

            rt.block_on(async move {
                let prompt = backend.oauth().begin_device_code().await?;
                println!(
                    "\nTo sign in, open {} and enter the code: {}\n",
                    prompt.verification_uri, prompt.user_code
                );
                println!("Waiting for you to complete sign-in in the browser...");
                let cache = backend.oauth().poll_for_token(&prompt).await?;
                cache.save(backend.cache_path())?;
                println!(
                    "Login successful. Tokens saved to {}",
                    backend.cache_path().display()
                );
                Ok::<(), anyhow::Error>(())
            })
        }
        other => Err(anyhow::anyhow!(
            "backend '{id}' has type '{other}', which does not support --login"
        )),
    }
}

/// Spawn the worker thread that polls backends and refreshes the snapshot.
fn spawn_poller(cfg: Config, events: EventSnapshot) {
    std::thread::Builder::new()
        .name("cnm-poller".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("failed to start tokio runtime: {e}");
                    return;
                }
            };

            rt.block_on(async move {
                let backends = build_backends(&cfg);
                if backends.is_empty() {
                    tracing::warn!("no usable backends configured; poller idle");
                }
                let interval = cfg.poll_interval();
                loop {
                    let now = Utc::now();
                    let window_start = now - Duration::hours(12);
                    let window_end = now + Duration::hours(24);

                    let mut all = Vec::new();
                    for backend in &backends {
                        match backend.fetch_events(window_start, window_end).await {
                            Ok(mut evs) => all.append(&mut evs),
                            Err(e) => {
                                tracing::warn!("backend '{}' fetch failed: {e}", backend.id())
                            }
                        }
                    }
                    tracing::debug!("polled {} event(s)", all.len());
                    if let Ok(mut guard) = events.lock() {
                        *guard = all;
                    }

                    tokio::time::sleep(interval).await;
                }
            });
        })
        .expect("spawn poller thread");
}

/// Re-evaluate the due set and reconcile the window.
fn recompute(ctx: &Rc<AppCtx>) {
    let now = Utc::now();
    let events = ctx.events.lock().map(|g| g.clone()).unwrap_or_default();

    let due = compute_due(&events, now, &ctx.engine_cfg, |id| {
        ctx.store.get(id).unwrap_or(ReminderState::Active)
    });
    tracing::debug!(
        "recompute: {} event(s) in snapshot, {} due",
        events.len(),
        due.len()
    );

    let window_ref = ctx.window.borrow();
    let Some(window) = window_ref.as_ref() else {
        return;
    };

    if due.is_empty() {
        if window.is_visible() {
            window.hide();
        }
        return;
    }

    let rows: Vec<ReminderRow> = due.iter().map(|d| to_row(d, now)).collect();
    window.update(rows);
    for d in &due {
        let _ = ctx.store.mark_shown(&d.event.id, now);
    }
    if !window.is_visible() {
        play_sound();
        window.show();
    }
}

/// Map an engine [`DueEvent`] to a UI [`ReminderRow`].
fn to_row(due: &DueEvent, now: DateTime<Utc>) -> ReminderRow {
    let e = &due.event;
    let subtitle = e.location.clone().filter(|loc| {
        // Don't repeat the meeting URL as a location line.
        due.event
            .meeting
            .as_ref()
            .map(|m| &m.url != loc)
            .unwrap_or(true)
    });
    ReminderRow {
        id: e.id.clone(),
        title: e.title.clone(),
        when_text: format_when(e.start, now),
        subtitle,
        snooze_options: due.snooze_options.clone(),
        meeting_label: e
            .meeting
            .as_ref()
            .map(|m| format!("Join {}", m.provider.display_name())),
    }
}

/// Human-friendly "09:00 — in 12 minutes" style time text.
fn format_when(start: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let local = start.with_timezone(&Local);
    let clock = local.format("%H:%M");
    let mins = (start - now).num_minutes();
    let rel = match mins {
        m if m > 1 => format!("in {m} minutes"),
        1 => "in 1 minute".to_string(),
        0 => "now".to_string(),
        -1 => "1 minute ago".to_string(),
        m => format!("{} minutes ago", -m),
    };
    format!("{clock} — {rel}")
}

/// Apply a user action, then schedule a recompute on the main loop.
fn handle_action(ctx: &Rc<AppCtx>, action: UiAction) {
    let now = Utc::now();
    match action {
        UiAction::Dismiss(id) => {
            let _ = ctx.store.dismiss(&id);
        }
        UiAction::DismissAll => {
            let events = ctx.events.lock().map(|g| g.clone()).unwrap_or_default();
            let due = compute_due(&events, now, &ctx.engine_cfg, |id| {
                ctx.store.get(id).unwrap_or(ReminderState::Active)
            });
            for d in &due {
                let _ = ctx.store.dismiss(&d.event.id);
            }
        }
        UiAction::Snooze(id, option) => {
            let events = ctx.events.lock().map(|g| g.clone()).unwrap_or_default();
            if let Some(event) = events.iter().find(|e| e.id == id) {
                let wake = resolve_snooze(option, event, now);
                let _ = ctx.store.snooze(&id, wake);
            } else {
                // Fall back to a relative snooze if the event left the snapshot.
                let _ = ctx.store.snooze(&id, now + Duration::minutes(5));
            }
        }
        UiAction::Join(id) => {
            let events = ctx.events.lock().map(|g| g.clone()).unwrap_or_default();
            if let Some(url) = events
                .iter()
                .find(|e| e.id == id)
                .and_then(|e| e.meeting.as_ref())
                .map(|m| m.url.clone())
            {
                open_url(&url);
            }
        }
    }

    // Recompute on idle so we don't rebuild widgets from within their own
    // signal handler.
    let weak: Weak<AppCtx> = Rc::downgrade(ctx);
    glib::idle_add_local_once(move || {
        if let Some(ctx) = weak.upgrade() {
            recompute(&ctx);
        }
    });
}

/// Open a meeting URL with the desktop's default handler.
fn open_url(url: &str) {
    match std::process::Command::new("xdg-open").arg(url).spawn() {
        Ok(_) => tracing::info!("opened meeting link"),
        Err(e) => tracing::error!("failed to launch xdg-open: {e}"),
    }
}

/// Play the desktop's "message" alert sound (best-effort, non-blocking).
fn play_sound() {
    match std::process::Command::new("canberra-gtk-play")
        .arg("--id=message")
        .spawn()
    {
        Ok(_) => tracing::debug!("played message sound"),
        Err(e) => tracing::error!("failed to play message sound: {e}"),
    }
}
