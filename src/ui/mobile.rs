use std::sync::Mutex;
use std::time::Duration;

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::model::Chip;
use crate::theme::{self, icon};

use super::{Action, App, LibTab, NpTab, Page, widgets};

static SEARCH: Mutex<Option<String>> = Mutex::new(None);

pub(super) fn take_search(app: &mut App) {
    if let Some(query) = SEARCH.lock().unwrap().take() {
        app.search_text = query.clone();
        app.go(Page::Search(query));
    }
}

pub(super) fn open_search(ctx: egui::Context) {
    let light = theme::light();
    std::thread::spawn(move || {
        if let Ok(Some(query)) = crate::android::request("search", Some(light), Duration::from_secs(600)) {
            let query = query.trim();
            if !query.is_empty() {
                *SEARCH.lock().unwrap() = Some(query.to_owned());
                ctx.request_repaint();
            }
        }
    });
}

pub(super) fn nav(app: &mut App, ui: &mut Ui) {
    let full = ui.max_rect();
    let tabs = [
        (icon::HOUSE, "Home", Page::Home),
        (icon::COMPASS, "Explore", Page::Explore),
        (icon::LIBRARY, "Library", Page::Library(LibTab::Playlists)),
        (icon::USERS, "Together", Page::Jam),
        (icon::SETTINGS, "Settings", Page::Settings),
    ];
    let width = full.width() / tabs.len() as f32;
    for (index, (glyph, label, page)) in tabs.into_iter().enumerate() {
        let left = full.left() + index as f32 * width;
        let slot = Rect::from_min_size(pos2(left, full.top()), vec2(width, full.height()));
        let selected = page == app.page || matches!((&page, &app.page), (Page::Library(_), Page::Library(_)));
        let response = ui.interact(slot, ui.scope_id().with(label), Sense::click());
        let color = if selected { app.accent_color() } else { theme::text_dim() };
        if selected {
            ui.painter().rect_filled(slot.shrink2(vec2(3.0, 5.0)), 12.0, theme::with_alpha(color, 0.10));
        }
        widgets::icon_at(ui, pos2(slot.center().x, slot.top() + 19.0), glyph, 19.0, color);
        ui.painter().text(pos2(slot.center().x, slot.bottom() - 7.0), Align2::CENTER_BOTTOM,
            label, theme::regular(11.0), color);
        if response.clicked() {
            app.actions.push(Action::Go(page));
        }
    }
}

pub(super) fn top_bar(app: &mut App, ui: &mut Ui) {
    let size = vec2(ui.available_width(), 46.0);
    let (full, _) = ui.allocate_exact_size(size, Sense::hover());
    let back = Rect::from_min_size(full.min, vec2(39.0, full.height()));
    if button(ui, back, icon::CHEVRON_LEFT, "Back", false) {
        app.nav_back();
    }
    if app.page == Page::Settings {
        return;
    }
    let field = Rect::from_min_max(pos2(back.right() + 4.0, full.top() + 2.0), pos2(full.right() - 12.0, full.bottom() - 2.0));
    ui.painter().rect_filled(field, 12.0, theme::glass(0.10));
    widgets::icon_at(ui, field.left_center() + vec2(19.0, 0.0), icon::SEARCH, 17.0, theme::text_dim());
    let label = if app.search_text.is_empty() { "Search music" } else { &app.search_text };
    widgets::text_at(ui, field.min + vec2(36.0, 12.0), label, theme::regular(14.0), theme::text_dim(), field.width() - 48.0);
    if ui.interact(field, ui.scope_id().with("native-search"), Sense::click()).clicked() {
        open_search(ui.ctx().clone());
    }
}

pub(super) fn home_header(app: &mut App, ui: &mut Ui) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::hover());
    let logo = Rect::from_center_size(row.left_center() + vec2(13.0, 0.0), vec2(26.0, 26.0));
    let png = crate::appicon::get("official").png(64);
    egui::Image::from_bytes("bytes://mobile-music-logo.png", png).paint_at(ui, logo);
    ui.painter().text(logo.right_center() + vec2(5.0, 0.0), Align2::LEFT_CENTER,
        "Music", theme::bold(19.0), theme::text());

    let slot = |index: f32| Rect::from_center_size(row.right_center() - vec2((index + 0.5) * 46.0, 0.0), vec2(46.0, row.height()));
    let account = slot(0.0);
    let avatar = Rect::from_center_size(account.center(), vec2(25.0, 25.0));
    if let Some(url) = app.account_avatar.as_ref().filter(|_| app.logged_in) {
        widgets::paint_remote(ui, url, avatar, avatar.width() / 2.0,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    } else {
        ui.painter().circle_filled(avatar.center(), avatar.width() / 2.0, theme::glass(0.12));
        if let Some(initial) = app.account_name.as_deref().filter(|_| app.logged_in).and_then(|name| name.chars().next()) {
            ui.painter().text(avatar.center(), Align2::CENTER_CENTER, initial, theme::semibold(14.0), theme::text());
        } else {
            widgets::icon_at(ui, avatar.center(), icon::USER, 16.0, theme::text());
        }
    }
    if widgets::named(ui.interact(account, ui.scope_id().with("home-account"), Sense::click()), "Account").clicked() {
        app.settings_tab = super::pages::SettingsTab::Account;
        app.actions.push(Action::Go(Page::Settings));
    }

    if button(ui, slot(1.0), icon::SEARCH, "Search music", false) {
        open_search(ui.ctx().clone());
    }

    let releases = slot(2.0);
    let center = releases.center();
    let stroke = Stroke::new(1.7, theme::text());
    let outline = [(-8.0, 4.0), (-6.0, 1.0), (-6.0, -2.0), (-5.0, -6.0), (-2.5, -8.0),
        (2.5, -8.0), (5.0, -6.0), (6.0, -2.0), (6.0, 1.0), (8.0, 4.0), (-8.0, 4.0)];
    ui.painter().add(egui::Shape::line(outline.into_iter().map(|(x, y)| center + vec2(x, y)).collect(), stroke));
    ui.painter().line_segment([center + vec2(-2.0, 7.0), center + vec2(2.0, 7.0)], stroke);
    if widgets::named(ui.interact(releases, ui.scope_id().with("home-releases"), Sense::click()), "New releases").clicked() {
        app.actions.push(Action::Go(Page::Explore));
    }
}

pub(super) fn home_chips(ui: &mut Ui, chips: &[Chip], actions: &mut Vec<Action>) {
    use egui::scroll_area::{ScrollBarVisibility, ScrollSource, State};
    use super::touch_scroll;

    const GAP: f32 = 8.0;
    let font = theme::semibold(13.0);
    let widths: Vec<_> = chips.iter().map(|chip| {
        ui.painter().layout_no_wrap(chip.title.clone(), font.clone(), Color32::PLACEHOLDER).size().x + 24.0
    }).collect();
    let viewport = ui.available_width();
    let width = widths.iter().sum::<f32>() + GAP * chips.len().saturating_sub(1) as f32;
    let salt = egui::Id::unique("mobile-home-chips");
    let id = ui.make_persistent_id(egui::IdSalt::new(salt));
    let mut area = egui::ScrollArea::horizontal().id_salt(salt)
        .scroll_source(ScrollSource::SCROLL_BAR | ScrollSource::MOUSE_WHEEL)
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden);
    if let Some(dx) = touch_scroll::take_delta(ui.ctx(), id) {
        let current = State::load(ui.ctx(), id).map_or(0.0, |state| state.clamped_offset().x);
        let next = (current - dx).clamp(0.0, (width - viewport).max(0.0));
        area = area.horizontal_scroll_offset(next);
    }
    let out = area.show(ui, |ui| ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (chip, width) in chips.iter().zip(widths) {
            let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
            let response = widgets::named(response, &chip.title);
            let (fill, text) = if chip.selected {
                (theme::text(), theme::on_text())
            } else {
                (theme::glass(if response.hovered() { 0.15 } else { 0.10 }), theme::text())
            };
            ui.painter().rect_filled(rect, 7.0, fill);
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, &chip.title, font.clone(), text);
            if response.clicked() {
                actions.push(Action::HomeChip(if chip.selected { None } else { chip.params.clone() }));
            }
        }
    }));
    if width > out.inner_rect.width() + 0.5 {
        touch_scroll::register(ui, id, out.inner_rect);
    }
}

fn button(ui: &mut Ui, rect: Rect, glyph: char, name: &str, active: bool) -> bool {
    let response = widgets::named(ui.interact(rect, ui.scope_id().with(name), Sense::click()), name).on_hover_text(name);
    let color = if active { theme::text_dim() } else { theme::text() };
    widgets::icon_at(ui, rect.center(), glyph, 21.0, color);
    response.clicked()
}

pub(super) fn player(app: &mut App, ui: &mut Ui) {
    let full = ui.max_rect();
    let Some(track) = app.current().cloned() else { return };
    let start = full.left() + 8.0;
    let end = full.right() - 8.0;
    let audio = app.audio.status.clone();
    let total = audio.duration().map(|duration| duration.as_secs_f32())
        .or(track.duration.map(|seconds| seconds as f32)).unwrap_or_default();
    let pos = audio.position_smooth().as_secs_f32();
    let progress = Rect::from_min_max(pos2(start, full.top()), pos2(end, full.top() + 13.0));
    let response = ui.interact(progress, ui.scope_id().with("seek"), Sense::click_and_drag());
    let line_y = progress.center().y;
    ui.painter().hline(progress.x_range(), line_y, egui::Stroke::new(2.0, theme::glass(0.20)));
    if total > 0.0 {
        let fraction = (pos / total).clamp(0.0, 1.0);
        let until = egui::lerp(progress.x_range(), fraction);
        ui.painter().hline(start..=until, line_y, egui::Stroke::new(2.0, app.accent_color()));
        if (response.clicked() || response.drag_stopped()) && let Some(point) = response.interact_pointer_pos() {
            let fraction = ((point.x - start) / (end - start).max(1.0)).clamp(0.0, 1.0);
            app.seek_to(Duration::from_secs_f32(fraction * total));
        }
    }
    let row = Rect::from_min_max(pos2(start, progress.bottom()), pos2(end, full.bottom()));
    let height = row.height();
    let art = Rect::from_center_size(pos2(start + 20.0, row.center().y), vec2(39.0, 39.0));
    widgets::paint_cover(ui, art, &track.thumbs, 6.0, 1.0);
    let width = if full.width() < 330.0 { 37.0 } else { 43.0 };
    let transport = row.right() - 4.0 * width;
    let title = pos2(art.right() + 9.0, row.center().y - 16.0);
    let title_width = (transport - title.x - 3.0).max(0.0);
    widgets::title_at(ui, title, &track.title, track.explicit, theme::bold(13.0), theme::text(), title_width);
    widgets::text_at(ui, title + vec2(0.0, 17.0), &track.artist_line(), theme::regular(11.0), theme::text_dim(), title_width);
    let song = Rect::from_min_max(row.min, pos2(transport, row.bottom()));
    if ui.interact(song, ui.scope_id().with("mobile-now-playing"), Sense::click()).clicked() {
        app.np_tab = NpTab::Art;
        app.actions.push(Action::OpenNowPlaying(true));
    }
    let slot = |index: u32| Rect::from_min_size(pos2(transport + index as f32 * width, row.top()), vec2(width, height));
    if button(ui, slot(0), icon::SKIP_BACK, "Previous", false) { app.prev(); }
    let (glyph, label) = if app.shows_pause_control() { (icon::PAUSE, "Pause") } else { (icon::PLAY, "Play") };
    if button(ui, slot(1), glyph, label, false) { app.toggle(); }
    if button(ui, slot(2), icon::SKIP_FORWARD, "Next", false) { app.next(); }
    if button(ui, slot(3), icon::LIST_MUSIC, "Play queue", app.queue_open) {
        app.queue_open = !app.queue_open;
    }
}

pub(super) fn system_back(app: &mut App, ctx: &egui::Context) {
    if egui::Popup::is_any_open(ctx) {
        egui::Popup::close_all(ctx);
    } else if app.notes_dialog.take().is_some() || app.dialog.take().is_some() {
        // Dismiss the top dialog before navigating the underlying page.
    } else if app.fullscreen {
        app.set_fullscreen(ctx, false);
    } else if app.np_open {
        app.np_open = false;
    } else if app.queue_open {
        app.queue_open = false;
    } else if app.selection.on(&app.page) {
        app.selection.clear();
    } else if !app.back.is_empty() {
        app.nav_back();
    } else {
        crate::android::background_from_back();
    }
}
