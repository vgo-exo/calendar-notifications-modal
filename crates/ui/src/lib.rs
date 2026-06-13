//! GTK4 reminder modal.
//!
//! The daemon owns the window for its whole lifetime; the daemon calls
//! [`ReminderWindow::update`] with the current view-model and
//! [`ReminderWindow::show`] / [`ReminderWindow::hide`] to present or dismiss it.
//! User interactions are reported back through a callback as [`UiAction`]s.

use std::cell::RefCell;
use std::rc::Rc;

use cnm_core::model::SnoozeOption;
use gtk4::prelude::*;
use gtk4::{
    Align, Application, ApplicationWindow, Box as GtkBox, Button, Label, ListBox, MenuButton,
    Orientation, Popover, ScrolledWindow, SelectionMode,
};

/// A single row to render in the modal, prepared by the daemon.
#[derive(Debug, Clone)]
pub struct ReminderRow {
    /// Stable event id (echoed back in [`UiAction`]s).
    pub id: String,
    /// Event title.
    pub title: String,
    /// Primary time text, e.g. "09:00 — in 12 minutes".
    pub when_text: String,
    /// Optional secondary line (location, etc.).
    pub subtitle: Option<String>,
    /// Context-aware snooze choices; empty hides the snooze button.
    pub snooze_options: Vec<SnoozeOption>,
    /// Meeting provider display name; `Some` shows a "Join" button.
    pub meeting_label: Option<String>,
}

/// An interaction the user performed in the modal.
#[derive(Debug, Clone)]
pub enum UiAction {
    Dismiss(String),
    DismissAll,
    Snooze(String, SnoozeOption),
    Join(String),
}

/// Wrapper around the GTK reminder window.
pub struct ReminderWindow {
    window: ApplicationWindow,
    list: ListBox,
    callback: Rc<dyn Fn(UiAction)>,
    rows: RefCell<Vec<ReminderRow>>,
}

impl ReminderWindow {
    /// Build the (initially hidden) modal window for `app`. `callback` receives
    /// every [`UiAction`] on the GTK main thread.
    pub fn new(app: &Application, callback: Rc<dyn Fn(UiAction)>) -> Rc<Self> {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Calendar reminders")
            .modal(true)
            .default_width(460)
            .default_height(520)
            .build();

        let root = GtkBox::new(Orientation::Vertical, 0);

        let header = Label::builder()
            .label("Upcoming events")
            .halign(Align::Start)
            .margin_top(12)
            .margin_bottom(8)
            .margin_start(12)
            .margin_end(12)
            .build();
        header.add_css_class("title-3");
        root.append(&header);

        let list = ListBox::new();
        list.set_selection_mode(SelectionMode::None);
        list.add_css_class("boxed-list");

        let scroller = ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .margin_start(12)
            .margin_end(12)
            .child(&list)
            .build();
        root.append(&scroller);

        let footer = GtkBox::new(Orientation::Horizontal, 8);
        footer.set_margin_top(12);
        footer.set_margin_bottom(12);
        footer.set_margin_start(12);
        footer.set_margin_end(12);
        footer.set_halign(Align::End);

        let dismiss_all = Button::with_label("Dismiss all");
        dismiss_all.add_css_class("destructive-action");
        footer.append(&dismiss_all);
        root.append(&footer);

        window.set_child(Some(&root));

        let this = Rc::new(Self {
            window,
            list,
            callback,
            rows: RefCell::new(Vec::new()),
        });

        {
            let cb = this.callback.clone();
            dismiss_all.connect_clicked(move |_| cb(UiAction::DismissAll));
        }

        // Closing the window via the WM is treated as "dismiss all" so events
        // don't silently disappear from tracking; the daemon decides what to do.
        {
            let cb = this.callback.clone();
            this.window.connect_close_request(move |win| {
                cb(UiAction::DismissAll);
                win.set_visible(false);
                gtk4::glib::Propagation::Stop
            });
        }

        this
    }

    /// Replace the list contents with `rows`.
    pub fn update(self: &Rc<Self>, rows: Vec<ReminderRow>) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for row in &rows {
            let widget = self.build_row(row);
            self.list.append(&widget);
        }
        *self.rows.borrow_mut() = rows;
    }

    /// Present the modal (raising it if already visible).
    pub fn show(&self) {
        self.window.present();
    }

    /// Hide the modal without destroying it.
    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    /// Whether the window is currently visible.
    pub fn is_visible(&self) -> bool {
        self.window.is_visible()
    }

    fn build_row(self: &Rc<Self>, row: &ReminderRow) -> GtkBox {
        let container = GtkBox::new(Orientation::Horizontal, 12);
        container.set_margin_top(8);
        container.set_margin_bottom(8);
        container.set_margin_start(8);
        container.set_margin_end(8);

        // Left: text block.
        let text = GtkBox::new(Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.set_halign(Align::Start);

        let title = Label::builder()
            .label(&row.title)
            .halign(Align::Start)
            .wrap(true)
            .build();
        title.add_css_class("heading");
        text.append(&title);

        let when = Label::builder()
            .label(&row.when_text)
            .halign(Align::Start)
            .build();
        when.add_css_class("dim-label");
        text.append(&when);

        if let Some(sub) = &row.subtitle {
            let subtitle = Label::builder()
                .label(sub)
                .halign(Align::Start)
                .wrap(true)
                .build();
            subtitle.add_css_class("dim-label");
            text.append(&subtitle);
        }
        container.append(&text);

        // Right: action buttons.
        let actions = GtkBox::new(Orientation::Horizontal, 6);
        actions.set_valign(Align::Center);

        if row.meeting_label.is_some() {
            let join = Button::with_label("Join");
            join.add_css_class("suggested-action");
            join.set_tooltip_text(row.meeting_label.as_deref());
            let cb = self.callback.clone();
            let id = row.id.clone();
            join.connect_clicked(move |_| cb(UiAction::Join(id.clone())));
            actions.append(&join);
        }

        if !row.snooze_options.is_empty() {
            let snooze = self.build_snooze_button(&row.id, &row.snooze_options);
            actions.append(&snooze);
        }

        let dismiss = Button::with_label("Dismiss");
        {
            let cb = self.callback.clone();
            let id = row.id.clone();
            dismiss.connect_clicked(move |_| cb(UiAction::Dismiss(id.clone())));
        }
        actions.append(&dismiss);

        container.append(&actions);
        container
    }

    fn build_snooze_button(self: &Rc<Self>, id: &str, options: &[SnoozeOption]) -> MenuButton {
        let popover = Popover::new();
        let menu = GtkBox::new(Orientation::Vertical, 2);
        menu.set_margin_top(4);
        menu.set_margin_bottom(4);
        menu.set_margin_start(4);
        menu.set_margin_end(4);

        for opt in options {
            let item = Button::with_label(&opt.label());
            item.add_css_class("flat");
            item.set_halign(Align::Fill);
            let cb = self.callback.clone();
            let id = id.to_string();
            let opt = *opt;
            let popover_weak = popover.downgrade();
            item.connect_clicked(move |_| {
                cb(UiAction::Snooze(id.clone(), opt));
                if let Some(p) = popover_weak.upgrade() {
                    p.popdown();
                }
            });
            menu.append(&item);
        }
        popover.set_child(Some(&menu));

        let button = MenuButton::builder().label("Snooze").popover(&popover).build();
        button
    }
}
