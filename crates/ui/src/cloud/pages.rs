//! The Backstage pages: File › Open and File › Save As, and the modal a
//! refused save puts up.
//!
//! Every side of a page is drawn only where the service reported the
//! capability behind it. A list under a heading the provider cannot fill reads
//! to a user as a broken window, so the heading goes too.

use craft_cloud::{Capability, Entry, EntryKind};
use egui::{RichText, Ui};

use crate::places::{Place, PlaceEntry, PlaceKind, PlacesAction, PlacesContent, PlacesIcon, PlacesLabels, PlacesNotice, PlacesPanel, PlacesStyle};

use super::{
    CloudApp, Folder, Link, Source, browse, column, dim, document_name, entry_row, fill, link, load_shared, open_entry, opens, parent_of,
    reload_active, retry_account, save_as, size_text, start, stem_of,
};

/// The character a label ends with when choosing it opens a dialog.
const ELLIPSIS: &str = "\u{2026}";

/// The account line, or the reason there is nothing to show yet.
fn header<A: CloudApp>(app: &mut A, ui: &mut Ui) {
    let style = app.style(ui);
    let mut retry = false;
    match (&app.cloud().account, &app.cloud().account_fault) {
        (_, Some(fault)) => {
            let text = fault.text(app.kind().noun);
            let again = app.tr("Try again");
            ui.horizontal(|ui| {
                dim(ui, &style, &text);
                if ui.button(again).clicked() {
                    retry = true;
                }
            });
        }
        (Some(account), None) => {
            let line = if account.display.is_empty() { app.tr("Signed in") } else { account.display.clone() };
            dim(ui, &style, &line);
        }
        (None, None) => {
            let waiting = app.tr("Connecting to your cloud account…");
            dim(ui, &style, &waiting);
        }
    }
    if retry {
        let cloud = app.cloud_mut();
        cloud.account_fault = None;
        cloud.asked_account = false;
        start(app);
    }
}

/// The offer to link an account, and the wait while the browser is open.
/// Returns true when it drew, because there is nothing else to show until an
/// account exists.
///
/// None of this is an error. The whole state means one thing: there is no
/// account yet, and linking one is a button away.
fn link_panel<A: CloudApp>(app: &mut A, ui: &mut Ui) -> bool {
    let Some(state) = app.cloud().link.clone() else { return false };
    let style = app.style(ui);
    let noun = app.kind().noun;
    let mut begin = false;
    let mut cancel = false;
    let mut browse_to: Option<String> = None;
    let words = LinkWords {
        none_yet: app.tr("No cloud account is linked yet."),
        why: app.tr(&format!("Link one to open and save {noun}s in your cloud storage.")),
        link_one: app.tr("Link a cloud account"),
        starting: app.tr("Starting…"),
        finish: app.tr("Finish linking in your browser."),
        waiting: app.tr("Waiting for your browser…"),
        open_in_browser: app.tr("Open in browser"),
        copy_link: app.tr("Copy link"),
        cancel: app.tr("Cancel"),
        again: app.tr("Try again"),
    };
    egui::Frame::NONE.fill(style.soft).corner_radius(6.0).inner_margin(12).show(ui, |ui| match state {
        Link::Offered => {
            ui.label(RichText::new(&words.none_yet).font(egui::FontId::new(13.0, style.bold.clone())).color(style.text));
            dim(ui, &style, &words.why);
            ui.add_space(8.0);
            begin = ui.button(&words.link_one).clicked();
        }
        Link::Starting => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                dim(ui, &style, &words.starting);
            });
        }
        Link::Waiting { url, .. } => {
            ui.label(RichText::new(&words.finish).font(egui::FontId::new(13.0, style.bold.clone())).color(style.text));
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                dim(ui, &style, &words.waiting);
            });
            ui.add_space(6.0);
            // A link alone is not enough. When the browser cannot be opened,
            // or opens the wrong thing, the only way out is carrying the
            // address across by hand, and a label cannot be selected.
            ui.horizontal(|ui| {
                if ui.button(&words.open_in_browser).clicked() {
                    browse_to = Some(url.clone());
                }
                if ui.button(&words.copy_link).clicked() {
                    ui.ctx().copy_text(url.clone());
                }
            });
            ui.add_space(4.0);
            if ui.link(&url).clicked() {
                browse_to = Some(url.clone());
            }
            ui.add_space(8.0);
            cancel = ui.button(&words.cancel).clicked();
        }
        Link::Failed(fault) => {
            ui.label(RichText::new(fault.text(noun)).font(egui::FontId::new(13.0, style.family.clone())).color(style.text));
            ui.add_space(8.0);
            begin = ui.button(&words.again).clicked();
        }
    });
    if let Some(url) = browse_to {
        app.open_url(&url);
    }
    if begin {
        link(app);
    } else if cancel {
        app.cloud_mut().link = Some(Link::Offered);
    }
    true
}

/// The panel's strings, read before it is drawn. The application is borrowed
/// mutably inside the frame, so nothing can be asked of it there.
struct LinkWords {
    none_yet: String,
    why: String,
    link_one: String,
    starting: String,
    finish: String,
    waiting: String,
    open_in_browser: String,
    copy_link: String,
    cancel: String,
    again: String,
}

/// The progress line for a read or a write in flight.
pub fn progress<A: CloudApp>(app: &A, ui: &mut Ui) {
    let Some(busy) = app.cloud().busy.clone() else { return };
    let style = app.style(ui);
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(15.0));
        ui.label(RichText::new(busy.text()).font(egui::FontId::new(12.5, style.family.clone())).color(style.text));
    });
}

/// File › Open: Recent, each linked cloud account, Shared with me, and This
/// computer. The panel performs no IO. The application feeds recents and
/// Browse; cloud files come through [`CloudApp`].
pub fn open_page<A: CloudApp>(app: &mut A, ui: &mut Ui) {
    start(app);
    let cloud_on = app.cloud().available();
    let account = app.cloud().account.clone();
    let link_state = app.cloud().link.clone();
    let account_fault = app.cloud().account_fault.clone();
    let mine = app.cloud().mine.clone();
    let shared = app.cloud().shared.clone();
    let busy = app.cloud().busy.as_ref().map(super::Busy::text);
    let status = app.cloud().status.clone();
    let noun = app.kind().noun;
    let recents = app.recents();
    let shared_offered = account.as_ref().is_some_and(|a| a.can(Capability::SharedWithMe));

    let mut places_owned = vec![OpenPlace { slot: OpenSlot::Recent, kind: PlaceKind::Recent, label: app.tr("Recent"), detail: None }];
    if cloud_on {
        let display = account.as_ref().map(|a| a.display.clone()).filter(|s| !s.is_empty());
        places_owned.push(OpenPlace { slot: OpenSlot::Mine, kind: PlaceKind::Account, label: app.tr("Cloud"), detail: display });
        if shared_offered {
            places_owned.push(OpenPlace { slot: OpenSlot::Shared, kind: PlaceKind::Account, label: app.tr("Shared with me"), detail: None });
        }
    }
    places_owned.push(OpenPlace { slot: OpenSlot::Local, kind: PlaceKind::Local, label: app.tr("This computer"), detail: None });

    {
        let n = places_owned.len();
        let place = &mut app.cloud_mut().open_place;
        if *place >= n {
            *place = n.saturating_sub(1);
        }
    }
    let selected = app.cloud().open_place;
    let slot = places_owned.get(selected).map(|p| p.slot).unwrap_or(OpenSlot::Recent);

    if cloud_on && matches!(slot, OpenSlot::Shared) && !shared.loading && !shared.loaded {
        load_shared(app);
    }

    let mut listed: Vec<Listed> = Vec::new();
    let mut crumbs_owned: Vec<String> = Vec::new();
    let mut crumb_paths: Vec<String> = Vec::new();
    let mut notice_act = NoticeAct::None;
    let mut content_kind = OpenContent::Entries;
    let mut message = String::new();
    let mut notice_title = String::new();
    let mut notice_body = String::new();
    let mut notice_action = String::new();

    match slot {
        OpenSlot::Recent => {
            if recents.is_empty() {
                content_kind = OpenContent::Message;
                message = app.tr(&format!("No recent {noun}s yet."));
            } else {
                for path in &recents {
                    let (name, folder) = split_name(path);
                    listed.push(Listed {
                        name,
                        folder: false,
                        detail: folder,
                        attention: false,
                        remote: false,
                        recent_path: Some(path.clone()),
                        entry: None,
                    });
                }
            }
        }
        OpenSlot::Mine | OpenSlot::Shared => fill_cloud_place(
            app,
            slot,
            &link_state,
            &account_fault,
            if matches!(slot, OpenSlot::Mine) { &mine } else { &shared },
            &mut listed,
            &mut crumbs_owned,
            &mut crumb_paths,
            &mut content_kind,
            &mut message,
            &mut notice_title,
            &mut notice_body,
            &mut notice_action,
            &mut notice_act,
        ),
        OpenSlot::Local => {
            content_kind = OpenContent::Notice;
            notice_title = app.tr("This computer");
            notice_body =
                app.tr(&format!("Open a {noun} with the system file dialog. Cloud folders the operating system already knows sit there too."));
            notice_action = app.tr("Browse…");
            notice_act = NoticeAct::Browse;
        }
    }

    let lbl_places = app.tr("Places");
    let lbl_add = app.tr("Link account");
    let lbl_browse = app.tr("Browse…");
    let lbl_loading = app.tr("Loading…");
    let labels = PlacesLabels { places: &lbl_places, add_place: &lbl_add, browse: &lbl_browse, loading: &lbl_loading };
    let places: Vec<Place<'_>> = places_owned.iter().map(|p| Place { kind: p.kind, label: &p.label, detail: p.detail.as_deref() }).collect();
    let entries: Vec<PlaceEntry<'_>> =
        listed.iter().map(|e| PlaceEntry { name: &e.name, folder: e.folder, detail: &e.detail, attention: e.attention, remote: e.remote }).collect();
    let notice = PlacesNotice {
        icon: match slot {
            OpenSlot::Local => PlacesIcon::Local,
            OpenSlot::Mine | OpenSlot::Shared => PlacesIcon::Account,
            OpenSlot::Recent => PlacesIcon::Recent,
        },
        title: &notice_title,
        body: &notice_body,
        action: if notice_action.is_empty() { None } else { Some(notice_action.as_str()) },
    };
    let content = match content_kind {
        OpenContent::Entries => PlacesContent::Entries(&entries),
        OpenContent::Loading => PlacesContent::Loading,
        OpenContent::Message => PlacesContent::Message(&message),
        OpenContent::Notice => PlacesContent::Notice(notice),
    };
    let crumb_refs: Vec<&str> = crumbs_owned.iter().map(String::as_str).collect();
    let add_place = cloud_on && account.as_ref().is_none_or(|a| !a.provider_linked);
    let style = PlacesStyle::from_visuals(ui.visuals());
    let busy_line = busy.clone();
    let status_line = status.clone();
    let text_color = style.text;
    let dim_color = style.weak;
    let response = PlacesPanel::new(&places, Some(selected), content, &style, labels)
        .crumbs(&crumb_refs)
        .add_place(add_place)
        .browse(true)
        .header(|ui| {
            if let Some(text) = &busy_line {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.label(RichText::new(text).size(12.5).color(text_color));
                });
            }
            if let Some(text) = &status_line {
                ui.label(RichText::new(text).size(12.0).color(dim_color));
            }
        })
        .show(ui);

    match response.action {
        Some(PlacesAction::Select(i)) => app.cloud_mut().open_place = i,
        Some(PlacesAction::Browse) => app.browse_local(),
        Some(PlacesAction::AddPlace) => super::link(app),
        Some(PlacesAction::Notice) => match notice_act {
            NoticeAct::Browse => app.browse_local(),
            NoticeAct::Link => super::link(app),
            NoticeAct::OpenUrl(url) => app.open_url(&url),
            NoticeAct::Retry => retry_account(app),
            NoticeAct::None => {}
        },
        Some(PlacesAction::Crumb(i)) => {
            if let Some(path) = crumb_paths.get(i) {
                browse(app, path);
            }
        }
        Some(PlacesAction::Activate(i)) => {
            if let Some(row) = listed.get(i) {
                if let Some(path) = row.recent_path.clone() {
                    app.open_recent(&path);
                } else if row.folder {
                    if let Some(entry) = &row.entry {
                        browse(app, &entry.path);
                    }
                } else if let Some(entry) = row.entry.clone() {
                    app.cloud_mut().source = match slot {
                        OpenSlot::Shared => Source::Shared,
                        _ => Source::Mine,
                    };
                    open_entry(app, entry, None);
                }
            }
        }
        None => {}
    }
}

#[derive(Clone, Copy)]
enum OpenSlot {
    Recent,
    Mine,
    Shared,
    Local,
}

struct OpenPlace {
    slot: OpenSlot,
    kind: PlaceKind,
    label: String,
    detail: Option<String>,
}

struct Listed {
    name: String,
    folder: bool,
    detail: String,
    attention: bool,
    remote: bool,
    recent_path: Option<String>,
    entry: Option<Entry>,
}

enum NoticeAct {
    None,
    Browse,
    Link,
    OpenUrl(String),
    Retry,
}

enum OpenContent {
    Entries,
    Loading,
    Message,
    Notice,
}

fn split_name(path: &str) -> (String, String) {
    match path.rsplit_once(['/', '\\']) {
        Some((folder, name)) => (name.to_owned(), folder.to_owned()),
        None => (path.to_owned(), String::new()),
    }
}

fn fill_cloud_place<A: CloudApp>(
    app: &A,
    slot: OpenSlot,
    link: &Option<Link>,
    account_fault: &Option<super::Fault>,
    folder: &Folder,
    listed: &mut Vec<Listed>,
    crumbs_owned: &mut Vec<String>,
    crumb_paths: &mut Vec<String>,
    content_kind: &mut OpenContent,
    message: &mut String,
    notice_title: &mut String,
    notice_body: &mut String,
    notice_action: &mut String,
    notice_act: &mut NoticeAct,
) {
    let noun = app.kind().noun;
    if let Some(state) = link {
        *content_kind = OpenContent::Notice;
        match state {
            Link::Offered => {
                *notice_title = app.tr("No cloud account is linked yet.");
                *notice_body = app.tr(&format!("Link one to open and save {noun}s in your cloud storage."));
                *notice_action = app.tr("Link a cloud account");
                *notice_act = NoticeAct::Link;
            }
            Link::Starting => {
                *content_kind = OpenContent::Loading;
            }
            Link::Waiting { url, .. } => {
                *notice_title = app.tr("Finish linking in your browser.");
                *notice_body = app.tr("Waiting for your browser…");
                *notice_action = app.tr("Open in browser");
                *notice_act = NoticeAct::OpenUrl(url.clone());
            }
            Link::Failed(fault) => {
                *notice_title = fault.text(noun);
                *notice_body = String::new();
                *notice_action = app.tr("Try again");
                *notice_act = NoticeAct::Link;
            }
        }
        return;
    }
    if let Some(fault) = account_fault {
        *content_kind = OpenContent::Notice;
        *notice_title = fault.text(noun);
        *notice_body = String::new();
        *notice_action = app.tr("Try again");
        *notice_act = NoticeAct::Retry;
        return;
    }
    if folder.loading && !folder.loaded {
        *content_kind = OpenContent::Loading;
        return;
    }
    if let Some(fault) = &folder.error {
        *content_kind = OpenContent::Message;
        *message = fault.text(noun);
        return;
    }
    if matches!(slot, OpenSlot::Mine) {
        crumbs_owned.push(app.tr("Home"));
        crumb_paths.push(String::new());
        if !folder.path.is_empty() {
            let mut acc = String::new();
            for part in folder.path.trim_matches('/').split('/') {
                if part.is_empty() {
                    continue;
                }
                acc = if acc.is_empty() { format!("/{part}") } else { format!("{acc}/{part}") };
                crumbs_owned.push(part.to_owned());
                crumb_paths.push(acc.clone());
            }
        }
    }
    let with_you = app.tr("Shared with you");
    let read_only = app.tr("read-only");
    for entry in &folder.entries {
        let is_folder = entry.kind == EntryKind::Folder;
        if !is_folder && !opens(entry, app.kind()) {
            continue;
        }
        let detail = if is_folder {
            String::new()
        } else if matches!(slot, OpenSlot::Shared) {
            let who = entry.shared_by.clone().unwrap_or_else(|| with_you.clone());
            if entry.writable { who } else { format!("{who} ({read_only})") }
        } else {
            size_text(entry.size)
        };
        listed.push(Listed {
            name: entry.name.clone(),
            folder: is_folder,
            detail,
            attention: false,
            remote: true,
            recent_path: None,
            entry: Some(entry.clone()),
        });
    }
    if listed.is_empty() && folder.loaded {
        *content_kind = OpenContent::Message;
        *message = if matches!(slot, OpenSlot::Shared) {
            app.tr(&format!("Nobody has shared a {noun} with you."))
        } else {
            app.tr(&format!("Nothing here that {} can open.", app.app_name()))
        };
    }
}

/// File › Save As, the cloud destination. Pick a folder, name the file, save.
/// Returns true when a save started, so a Backstage view can return to the
/// document.
pub fn save_page<A: CloudApp>(app: &mut A, ui: &mut Ui) -> bool {
    let style = app.style(ui);
    let kind = app.kind();
    start(app);
    // The destination above already names the account. This line is here to
    // say when something is wrong with it.
    if app.cloud().account.is_none() || app.cloud().account_fault.is_some() {
        header(app, ui);
        ui.add_space(8.0);
    }
    if link_panel(app, ui) {
        return false;
    }
    progress(app, ui);
    if app.cloud().save_name.is_empty() {
        let suggested = app.active().map(|uid| app.title(uid)).unwrap_or_default();
        app.cloud_mut().save_name = document_name(&suggested, kind);
    }
    let folder = app.cloud().mine.clone();
    let heading = app.tr("Folder");
    let (home, up, loading) = (app.tr("Home"), app.tr("Up"), app.tr("Loading…"));
    let only_folder = app.tr(&format!("No folders here. The {} saves in this one.", kind.noun));
    let mut go: Option<String> = None;
    column(ui, &style, &heading, |ui| {
        let here = if folder.path.is_empty() { home } else { folder.path.clone() };
        ui.horizontal(|ui| {
            if let Some(parent) = parent_of(&folder.path)
                && ui.button(&up).clicked()
            {
                go = Some(parent);
            }
            dim(ui, &style, &here);
        });
        ui.add_space(4.0);
        if folder.loading {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                dim(ui, &style, &loading);
            });
            return;
        }
        if let Some(fault) = &folder.error {
            dim(ui, &style, &fault.text(kind.noun));
            return;
        }
        let mut folders = 0;
        for entry in folder.entries.iter().filter(|e| e.kind == EntryKind::Folder) {
            folders += 1;
            if entry_row(app, ui, entry, "", 0.0).clicked() {
                go = Some(entry.path.clone());
            }
        }
        if folders == 0 && folder.loaded {
            dim(ui, &style, &only_folder);
        }
    });
    if let Some(path) = go {
        browse(app, &path);
    }
    ui.add_space(12.0);
    let (file_name, save_label) = (app.tr("File name"), app.tr("Save to the cloud"));
    let mut asked = false;
    ui.horizontal(|ui| {
        ui.label(file_name);
        let typing = ui.add(egui::TextEdit::singleline(&mut app.cloud_mut().save_name).desired_width(280.0));
        asked = typing.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    });
    ui.add_space(10.0);
    let ready = app.cloud().busy.is_none() && !document_name(&app.cloud().save_name, kind).is_empty();
    if ui.add_enabled(ready, egui::Button::new(save_label)).clicked() {
        asked = true;
    }
    if let Some(note) = app.save_note() {
        ui.add_space(8.0);
        dim(ui, &style, &note);
    }
    // A provider with no conditional writes cannot refuse one, so a save here
    // can land on top of somebody else's. Said plainly, because the user is the
    // only one who can decide whether that matters.
    if app.cloud().account.as_ref().is_some_and(|a| !a.can(Capability::Etags)) {
        let warning = app.tr("This cloud account cannot detect a save that would overwrite somebody else's changes.");
        ui.add_space(4.0);
        dim(ui, &style, &warning);
    }
    if asked && ready && app.ready_to_save() {
        let (folder, name) = (app.cloud().mine.path.clone(), app.cloud().save_name.clone());
        match save_as(app, &folder, &name) {
            Ok(_) => return true,
            Err(error) => app.say(error),
        }
    }
    false
}

/// A save refused because the stored copy had moved on.
///
/// Three ways out, and none of them throws anything away without saying so.
/// Reloading is the one a user usually wants, so it comes first. The text says
/// plainly that it discards their edits.
pub fn conflict_modal<A: CloudApp>(app: &mut A, ctx: &egui::Context) {
    let Some(conflict) = app.cloud().conflict.clone() else { return };
    let noun = app.kind().noun;
    let words = ConflictWords {
        title: app.tr(&format!("This {noun} was not saved")),
        what: fill(
            app,
            "Somebody else saved {} while you were editing it. Nothing was written: saving now would overwrite their work.",
            &conflict.entry.name,
        ),
        yours: app.tr("Your own changes are still open here. Reloading replaces them with the stored copy."),
        reload: app.tr("Reload from the cloud"),
        copy: app.tr(&format!("Save as a copy{ELLIPSIS}")),
        keep: app.tr("Keep editing"),
    };
    let mut reload = false;
    let mut copy = false;
    let mut dismiss = false;
    egui::Modal::new(egui::Id::new("craft-ui/cloud/conflict")).show(ctx, |ui| {
        let style = app.style(ui);
        ui.set_width(430.0);
        ui.label(RichText::new(&words.title).font(egui::FontId::new(15.0, style.bold.clone())));
        ui.add_space(8.0);
        ui.add(egui::Label::new(&words.what).wrap());
        ui.add_space(6.0);
        ui.add(egui::Label::new(&words.yours).wrap());
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            reload = ui.button(&words.reload).clicked();
            copy = ui.button(&words.copy).clicked();
            dismiss = ui.button(&words.keep).clicked();
        });
    });
    if reload {
        app.cloud_mut().conflict = None;
        if let Err(error) = reload_active(app) {
            app.say(error);
        }
    } else if copy {
        let name = document_name(&format!("{} (copy)", stem_of(&conflict.entry.name)), app.kind());
        let cloud = app.cloud_mut();
        cloud.conflict = None;
        cloud.save_name = name;
        app.go_to_save_as();
    } else if dismiss {
        app.cloud_mut().conflict = None;
    }
}

/// The modal's strings, read before it is drawn.
struct ConflictWords {
    title: String,
    what: String,
    yours: String,
    reload: String,
    copy: String,
    keep: String,
}
