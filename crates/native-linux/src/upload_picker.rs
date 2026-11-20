use crate::ui::{dialog_guard, dismiss, job, label};
use gtk::prelude::*;
use hshell_sftp::{list_local_upload, LocalDirectory};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};

struct Picker {
    dialog: gtk::Dialog,
    alive: Rc<Cell<bool>>,
    address: gtk::Entry,
    home: gtk::Button,
    up: gtk::Button,
    go: gtk::Button,
    status: gtk::Label,
    tree: gtk::TreeView,
    model: gtk::ListStore,
    directory: RefCell<Option<LocalDirectory>>,
    loading: Cell<bool>,
}

impl Picker {
    fn update_selection(&self) {
        let count = self.tree.selection().count_selected_rows();
        self.dialog
            .set_response_sensitive(gtk::ResponseType::Accept, count > 0 && !self.loading.get());
        if self.loading.get() {
            return;
        }
        self.status.set_text(&if count > 0 {
            format!("已选 {count} 项")
        } else if self.model.iter_n_children(None) > 0 {
            "请选择要上传的项目".into()
        } else {
            "此目录没有可上传的项目".into()
        });
    }

    fn navigate(self: &Rc<Self>, path: Option<PathBuf>) {
        if self.loading.replace(true) {
            return;
        }
        self.update_selection();
        for button in [&self.home, &self.up, &self.go] {
            button.set_sensitive(false);
        }
        self.status.set_text("正在读取本地目录…");
        let weak = Rc::downgrade(self);
        job(
            move || list_local_upload(path),
            move |result| {
                let Some(picker) = weak.upgrade().filter(|picker| picker.alive.get()) else {
                    return;
                };
                let mut error = None;
                match result {
                    Ok(directory) => {
                        picker.address.set_text(&directory.path.to_string_lossy());
                        picker.model.clear();
                        for item in &directory.entries {
                            let icon = if item.directory {
                                "folder-symbolic"
                            } else {
                                "text-x-generic-symbolic"
                            };
                            picker
                                .model
                                .insert_with_values(None, &[(0, &icon), (1, &item.name)]);
                        }
                        picker.directory.replace(Some(directory));
                        picker.tree.grab_focus();
                    }
                    Err(reason) => error = Some(reason),
                }
                picker.loading.set(false);
                picker.home.set_sensitive(true);
                picker.go.set_sensitive(true);
                picker.up.set_sensitive(
                    picker
                        .directory
                        .borrow()
                        .as_ref()
                        .is_some_and(|directory| directory.parent.is_some()),
                );
                picker.update_selection();
                if let Some(error) = error {
                    picker.status.set_text(&error);
                }
            },
        );
    }
}

pub fn choose(
    window: &gtk::ApplicationWindow,
    target: &Path,
    previous: Option<PathBuf>,
) -> (Option<PathBuf>, Vec<PathBuf>) {
    let dialog = gtk::Dialog::with_buttons(
        Some("选择上传项目"),
        Some(window),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[
            ("取消", gtk::ResponseType::Cancel),
            ("上传", gtk::ResponseType::Accept),
        ],
    );
    dialog.set_default_size(620, 410);
    let content = dialog.content_area();
    content.set_border_width(12);
    content.set_spacing(8);
    let destination = label(&format!("上传到：{}", target.display()), "dim");
    destination.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    content.pack_start(&destination, false, false, 0);
    let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let home = gtk::Button::with_label("主目录");
    let up = gtk::Button::with_label("上一级");
    let go = gtk::Button::with_label("前往");
    let address = gtk::Entry::new();
    address.set_placeholder_text(Some("输入本地目录"));
    address.set_tooltip_text(Some("本地目录"));
    navigation.pack_start(&home, false, false, 0);
    navigation.pack_start(&up, false, false, 0);
    navigation.pack_start(&address, true, true, 0);
    navigation.pack_start(&go, false, false, 0);
    content.pack_start(&navigation, false, false, 0);
    content.pack_start(
        &label("Ctrl 多选 · Shift 连选 · 双击文件夹进入", "dim"),
        false,
        false,
        0,
    );
    let model = gtk::ListStore::new(&[String::static_type(), String::static_type()]);
    let tree = gtk::TreeView::with_model(&model);
    tree.selection().set_mode(gtk::SelectionMode::Multiple);
    tree.set_headers_visible(false);
    tree.set_search_column(1);
    let column = gtk::TreeViewColumn::new();
    let icon = gtk::CellRendererPixbuf::new();
    let name = gtk::CellRendererText::new();
    TreeViewColumnExt::pack_start(&column, &icon, false);
    TreeViewColumnExt::pack_start(&column, &name, true);
    TreeViewColumnExt::add_attribute(&column, &icon, "icon-name", 0);
    TreeViewColumnExt::add_attribute(&column, &name, "text", 1);
    tree.append_column(&column);
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scroll.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scroll.add(&tree);
    content.pack_start(&scroll, true, true, 0);
    let status = label("", "dim");
    status.set_line_wrap(true);
    content.pack_start(&status, false, false, 0);
    let picker = Rc::new(Picker {
        alive: dialog_guard(&dialog),
        dialog,
        address,
        home,
        up,
        go,
        tree,
        model,
        status,
        directory: RefCell::new(None),
        loading: Cell::new(false),
    });
    let weak = Rc::downgrade(&picker);
    picker.tree.selection().connect_changed(move |_| {
        if let Some(picker) = weak.upgrade() {
            picker.update_selection();
        }
    });
    let weak = Rc::downgrade(&picker);
    picker.tree.connect_row_activated(move |_, row, _| {
        if let Some(picker) = weak.upgrade() {
            let next = picker.directory.borrow().as_ref().and_then(|directory| {
                let item = directory.entries.get(*row.indices().first()? as usize)?;
                item.directory.then(|| item.path.clone())
            });
            if next.is_some() {
                picker.navigate(next);
            }
        }
    });
    let weak = Rc::downgrade(&picker);
    picker.home.connect_clicked(move |_| {
        if let Some(picker) = weak.upgrade() {
            picker.navigate(None);
        }
    });
    let weak = Rc::downgrade(&picker);
    picker.up.connect_clicked(move |_| {
        if let Some(picker) = weak.upgrade() {
            let parent = picker
                .directory
                .borrow()
                .as_ref()
                .and_then(|directory| directory.parent.clone());
            if parent.is_some() {
                picker.navigate(parent);
            }
        }
    });
    let weak = Rc::downgrade(&picker);
    let go = move || {
        if let Some(picker) = weak.upgrade() {
            let path = picker.address.text();
            picker.navigate((!path.is_empty()).then(|| PathBuf::from(path.as_str())));
        }
    };
    let go_button = go.clone();
    picker.go.connect_clicked(move |_| go_button());
    picker.address.connect_activate(move |_| go());
    picker.dialog.show_all();
    picker.tree.grab_focus();
    picker.navigate(previous.clone());
    let accepted = picker.dialog.run() == gtk::ResponseType::Accept;
    let directory = picker.directory.borrow();
    let location = directory
        .as_ref()
        .map(|directory| directory.path.clone())
        .or(previous);
    let files = if accepted && picker.alive.get() && !picker.loading.get() {
        directory
            .as_ref()
            .map(|directory| {
                picker
                    .tree
                    .selection()
                    .selected_rows()
                    .0
                    .iter()
                    .filter_map(|row| {
                        directory
                            .entries
                            .get(*row.indices().first()? as usize)
                            .map(|item| item.path.clone())
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    dismiss(&picker.dialog, &picker.alive);
    (location, files)
}
