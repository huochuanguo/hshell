use crate::ui::{dialog_guard, dismiss, entry, job, label, Client, Phase};
use gtk::prelude::*;
use ssh_core::Snippet;
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

const NEW: gtk::ResponseType = gtk::ResponseType::Other(1);
const EDIT: gtk::ResponseType = gtk::ResponseType::Other(2);
const DELETE: gtk::ResponseType = gtk::ResponseType::Other(3);
const INSERT: gtk::ResponseType = gtk::ResponseType::Other(4);

pub fn open(client: &Rc<Client>, button: &gtk::Button, path: PathBuf) {
    button.set_sensitive(false);
    let weak = Rc::downgrade(client);
    let button = button.clone();
    let source = path.clone();
    job(
        move || ssh_core::load_snippets(&source),
        move |result| {
            if let Some(client) = weak.upgrade() {
                match result {
                    Ok(list) => show(&client, &path, list),
                    Err(error) => client.error(&error),
                }
            }
            button.set_sensitive(true);
        },
    );
}

fn show(client: &Rc<Client>, path: &Path, snippets: Vec<Snippet>) {
    let dialog = gtk::Dialog::with_buttons(
        Some("命令片段"),
        Some(&client.window),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[
            ("关闭", gtk::ResponseType::Close),
            ("新建", NEW),
            ("编辑", EDIT),
            ("删除", DELETE),
            ("插入到终端", INSERT),
        ],
    );
    dialog.set_default_size(530, -1);
    let alive = dialog_guard(&dialog);
    let content = dialog.content_area();
    content.set_border_width(14);
    content.set_spacing(8);
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("按名称、分组或命令搜索"));
    let list = gtk::ComboBoxText::new();
    let preview = label("", "dim");
    preview.set_selectable(true);
    preview.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let error = label("", "error");
    error.set_line_wrap(true);
    content.pack_start(&search, false, false, 0);
    content.pack_start(&list, false, false, 0);
    content.pack_start(&preview, false, false, 0);
    content.pack_start(&error, false, false, 0);
    let snippets = Rc::new(RefCell::new(snippets));
    let data = snippets.clone();
    let preview_ = preview.clone();
    let weak_dialog = dialog.downgrade();
    let weak_client = Rc::downgrade(client);
    list.connect_changed(move |list| {
        let (Some(dialog), Some(client)) = (weak_dialog.upgrade(), weak_client.upgrade()) else {
            return;
        };
        let id = list.active_id();
        let data = data.borrow();
        let selected = data
            .iter()
            .find(|item| id.as_deref() == Some(item.id.as_str()));
        preview_.set_text(selected.map_or("没有匹配的命令片段", |item| &item.command));
        dialog.set_response_sensitive(EDIT, selected.is_some());
        dialog.set_response_sensitive(DELETE, selected.is_some());
        dialog.set_response_sensitive(INSERT, selected.is_some() && running(&client));
    });
    let list_ = list.clone();
    let data = snippets.clone();
    search.connect_changed(move |entry| populate(&list_, &data.borrow(), &entry.text(), None));
    dialog.set_response_sensitive(INSERT, false);
    dialog.set_response_sensitive(EDIT, false);
    dialog.set_response_sensitive(DELETE, false);
    dialog.show_all();
    populate(&list, &snippets.borrow(), "", None);
    search.grab_focus();
    let mut command = None;
    loop {
        let response = dialog.run();
        if !alive.get() {
            break;
        }
        let id = list.active_id();
        let selected = snippets
            .borrow()
            .iter()
            .find(|item| id.as_deref() == Some(item.id.as_str()))
            .cloned();
        let mut next = snippets.borrow().clone();
        let select_after = match response {
            NEW | EDIT => {
                let previous = selected.as_ref().filter(|_| response == EDIT);
                let Some(item) = editor(&dialog, previous) else {
                    continue;
                };
                let id = item.id.clone();
                if let Some(index) = next.iter().position(|existing| existing.id == item.id) {
                    next[index] = item;
                } else {
                    next.push(item);
                }
                Some(id)
            }
            DELETE => {
                let Some(item) = selected else { continue };
                let confirm = gtk::MessageDialog::new(
                    Some(&dialog),
                    gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
                    gtk::MessageType::Question,
                    gtk::ButtonsType::None,
                    "删除命令片段？",
                );
                confirm.set_secondary_text(Some(&item.name));
                confirm.add_buttons(&[
                    ("取消", gtk::ResponseType::Cancel),
                    ("删除", gtk::ResponseType::Accept),
                ]);
                let live = dialog_guard(&confirm);
                let accepted = confirm.run() == gtk::ResponseType::Accept;
                dismiss(&confirm, &live);
                if !accepted {
                    continue;
                }
                next.retain(|existing| existing.id != item.id);
                None
            }
            INSERT => {
                if !running(client) {
                    error.set_text("请先打开一个运行中的 SSH 会话。");
                    continue;
                }
                command = selected.map(|item| item.command);
                break;
            }
            _ => break,
        };
        if !alive.get() {
            break;
        }
        match ssh_core::save_snippets(path, &next) {
            Ok(()) => {
                snippets.replace(next);
                search.set_text("");
                populate(&list, &snippets.borrow(), "", select_after.as_deref());
                error.set_text("");
            }
            Err(failure) => error.set_text(&failure),
        }
    }
    dismiss(&dialog, &alive);
    if let (Some(command), Some(tab)) = (command, client.active()) {
        if tab.phase.get() == Phase::Running && tab.terminal.insert_snippet(&command) {
            tab.terminal.widget.grab_focus();
            client.notice("命令已插入终端，请确认后按 Enter");
        }
    }
}

fn running(client: &Client) -> bool {
    client
        .active()
        .is_some_and(|tab| tab.phase.get() == Phase::Running)
}

fn populate(list: &gtk::ComboBoxText, data: &[Snippet], query: &str, selected: Option<&str>) {
    let query = query.trim().to_lowercase();
    let mut matches: Vec<_> = data
        .iter()
        .filter(|item| {
            [&item.name, &item.group, &item.command]
                .iter()
                .any(|text| text.to_lowercase().contains(&query))
        })
        .collect();
    matches.sort_by(|a, b| a.group.cmp(&b.group).then_with(|| a.name.cmp(&b.name)));
    list.remove_all();
    for item in matches {
        let group = if item.group.is_empty() {
            "未分组"
        } else {
            &item.group
        };
        list.append(Some(&item.id), &format!("{group} · {}", item.name));
    }
    if !selected.is_some_and(|id| list.set_active_id(Some(id))) {
        list.set_active(Some(0));
    }
}

fn editor(parent: &gtk::Dialog, item: Option<&Snippet>) -> Option<Snippet> {
    let dialog = gtk::Dialog::with_buttons(
        Some(if item.is_some() {
            "编辑命令片段"
        } else {
            "新建命令片段"
        }),
        Some(parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[
            ("取消", gtk::ResponseType::Cancel),
            ("保存片段", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_default_size(480, -1);
    let alive = dialog_guard(&dialog);
    let content = dialog.content_area();
    content.set_border_width(14);
    content.set_spacing(6);
    let name = entry("名称", 80);
    let group = entry("分组（可留空）", 40);
    let command = gtk::TextView::new();
    command.set_monospace(true);
    command.set_wrap_mode(gtk::WrapMode::WordChar);
    let buffer = command.buffer()?;
    if let Some(item) = item {
        name.set_text(&item.name);
        group.set_text(&item.group);
        buffer.set_text(&item.command);
    }
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scroll.set_min_content_height(72);
    scroll.add(&command);
    content.pack_start(&label("名称", ""), false, false, 0);
    content.pack_start(&name, false, false, 0);
    content.pack_start(&label("分组", ""), false, false, 0);
    content.pack_start(&group, false, false, 0);
    content.pack_start(
        &label("命令 · 仅单行，插入后由你按 Enter 执行", ""),
        false,
        false,
        0,
    );
    content.pack_start(&scroll, true, true, 0);
    let error = label("", "error");
    content.pack_start(&error, false, false, 0);
    dialog.show_all();
    name.grab_focus();
    let mut result = None;
    while dialog.run() == gtk::ResponseType::Accept && alive.get() {
        let title = name.text().trim().to_owned();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .unwrap_or_default();
        if title.is_empty() {
            error.set_text("请输入名称");
            continue;
        }
        if let Err(invalid) = ssh_core::validate_snippet_text(&text) {
            error.set_text(&invalid);
            continue;
        }
        result = Some(Snippet {
            id: item.map_or_else(|| uuid::Uuid::new_v4().to_string(), |item| item.id.clone()),
            name: title,
            group: group.text().trim().to_owned(),
            command: text.to_string(),
        });
        break;
    }
    dismiss(&dialog, &alive);
    result
}
