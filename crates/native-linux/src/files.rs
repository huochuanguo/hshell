//! On-demand native file browser; bytes never pass through GTK.
use crate::ui::{dialog_guard, dismiss, entry, icon_button, job, label};
use gtk::{glib, prelude::*};
use hshell_sftp::{
    BrowseRequest, Browser, BrowserEvent, Direction, Entry, Transfer, TransferEvent, WorkerControl,
};
use ssh_core::{Profile, SubsystemAuthentication};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
enum FileKind {
    Folder,
    Link,
    File,
    Code,
    Archive,
    Image,
}

impl FileKind {
    const ALL: [Self; 6] = [
        Self::Folder,
        Self::Link,
        Self::File,
        Self::Code,
        Self::Archive,
        Self::Image,
    ];

    fn of(entry: &Entry) -> Self {
        if entry.symlink {
            return Self::Link;
        }
        if entry.directory {
            return Self::Folder;
        }
        let extension = entry
            .name
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match extension.as_str() {
            "zip" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "tar" | "7z" | "rar" | "deb" | "rpm" => {
                Self::Archive
            }
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => Self::Image,
            "sh" | "bash" | "py" | "rs" | "js" | "ts" | "jsx" | "tsx" | "go" | "c" | "h"
            | "cpp" | "java" | "json" | "yaml" | "yml" | "toml" | "ini" | "conf" | "xml"
            | "sql" | "css" | "html" => Self::Code,
            _ => Self::File,
        }
    }

    fn icon_name(self) -> &'static str {
        match self {
            Self::Folder => "folder-symbolic",
            Self::Link => "insert-link-symbolic",
            Self::File => "text-x-generic-symbolic",
            Self::Code => "text-x-generic-symbolic",
            Self::Archive => "package-x-generic-symbolic",
            Self::Image => "image-x-generic-symbolic",
        }
    }

    fn icon_color(self) -> &'static str {
        match self {
            Self::Folder => "#d6b578",
            Self::Link => "#8fc9c2",
            Self::File => "#a2aab7",
            Self::Code => "#9abceb",
            Self::Archive => "#b7a4d8",
            Self::Image => "#a3c69b",
        }
    }

    fn name_color(self) -> &'static str {
        match self {
            Self::Folder | Self::Link => self.icon_color(),
            _ => "#dce1e9",
        }
    }

    fn name_weight(self) -> i32 {
        if matches!(self, Self::Folder) {
            500
        } else {
            400
        }
    }
}

fn file_icons(scale: i32) -> [Option<gtk::cairo::Surface>; 6] {
    let theme = gtk::IconTheme::default();
    // Each category shares one tinted bitmap. Directory batches only retain
    // references; no icon loading or recoloring happens while drawing rows.
    FileKind::ALL.map(|kind| {
        let theme = theme.as_ref()?;
        let info = theme
            .lookup_icon_for_scale(
                kind.icon_name(),
                18,
                scale,
                gtk::IconLookupFlags::FORCE_SYMBOLIC | gtk::IconLookupFlags::FORCE_SIZE,
            )
            .or_else(|| {
                theme.lookup_icon_for_scale(
                    "text-x-generic-symbolic",
                    18,
                    scale,
                    gtk::IconLookupFlags::FORCE_SYMBOLIC | gtk::IconLookupFlags::FORCE_SIZE,
                )
            })?;
        let foreground = gtk::gdk::RGBA::parse(kind.icon_color()).ok()?;
        info.load_symbolic(&foreground, None, None, None)
            .map(|(icon, _)| icon)
            .or_else(|_| info.load_icon())
            .ok()
            .and_then(|icon| {
                gtk::gdk::prelude::GdkPixbufExt::create_surface(
                    &icon,
                    scale,
                    None::<&gtk::gdk::Window>,
                )
            })
    })
}

#[derive(PartialEq)]
enum State {
    Queued,
    Running,
    Paused,
    Complete,
}
struct Task {
    transfer: Transfer,
    state: State,
    control: Option<WorkerControl>,
    receiver: Option<mpsc::Receiver<TransferEvent>>,
    row: gtk::ListBoxRow,
    detail: gtk::Label,
    progress: gtk::ProgressBar,
    action: gtk::Button,
    remove: gtk::Button,
    last: (u64, Instant),
}

pub struct FilePane {
    pub root: gtk::Box,
    window: gtk::ApplicationWindow,
    profiles_path: PathBuf,
    state_path: PathBuf,
    profile: RefCell<Option<Profile>>,
    browser: RefCell<Option<Browser>>,
    receiver: RefCell<Option<mpsc::Receiver<BrowserEvent>>>,
    generation: Cell<u64>,
    loading: Cell<bool>,
    path: RefCell<PathBuf>,
    address: gtk::Entry,
    host: gtk::Label,
    status: gtk::Label,
    model: gtk::ListStore,
    icons: [Option<gtk::cairo::Surface>; 6],
    tree: gtk::TreeView,
    entries: RefCell<Vec<Entry>>,
    tasks: RefCell<Vec<Task>>,
    queue: gtk::ListBox,
    queue_title: gtk::Label,
    restoring: Cell<bool>,
    creating: Cell<usize>,
    closed: Cell<bool>,
    wake: glib::Sender<()>,
    polling: Cell<bool>,
    poll_dirty: Cell<bool>,
}

impl FilePane {
    #[allow(deprecated)]
    pub fn new(window: &gtk::ApplicationWindow, profiles_path: PathBuf) -> Rc<Self> {
        let (wake, wake_receiver) = glib::MainContext::channel(glib::Priority::DEFAULT_IDLE);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.style_context().add_class("file-pane");
        root.set_size_request(340, -1);
        root.set_no_show_all(true);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let host = label("远程文件", "heading");
        host.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        heading.pack_start(&host, true, true, 0);
        let close = icon_button("window-close-symbolic", "收起文件栏；后台传输继续");
        heading.pack_end(&close, false, false, 0);
        root.pack_start(&heading, false, false, 0);
        let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let up = icon_button("go-up-symbolic", "上一级目录");
        let refresh = icon_button("view-refresh-symbolic", "刷新 / 重新连接");
        let address = entry("远程目录", 4096);
        address.set_hexpand(true);
        address.set_text(".");
        navigation.pack_start(&up, false, false, 0);
        navigation.pack_start(&address, true, true, 0);
        navigation.pack_start(&refresh, false, false, 0);
        root.pack_start(&navigation, false, false, 0);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        let upload = gtk::Button::with_label("上传");
        let download = gtk::Button::with_label("下载");
        let mkdir = icon_button("folder-new-symbolic", "新建目录");
        let rename = icon_button("document-edit-symbolic", "重命名");
        let delete = icon_button("edit-delete-symbolic", "删除文件或空目录");
        for button in [&upload, &download, &mkdir, &rename, &delete] {
            actions.pack_start(button, false, false, 0);
        }
        root.pack_start(&actions, false, false, 0);
        let columns = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        columns.pack_start(&label("名称", "dim"), true, true, 4);
        columns.pack_end(&label("大小", "dim"), false, false, 8);
        root.pack_start(&columns, false, false, 0);
        let model = gtk::ListStore::new(&[
            gtk::cairo::Surface::static_type(),
            String::static_type(),
            String::static_type(),
            u32::static_type(),
            String::static_type(),
            i32::static_type(),
        ]);
        let tree = gtk::TreeView::with_model(&model);
        tree.set_search_column(1);
        tree.set_headers_visible(false);
        tree.set_enable_tree_lines(false);
        let name_column = gtk::TreeViewColumn::new();
        name_column.set_title("名称");
        name_column.set_expand(true);
        name_column.set_resizable(true);
        let image = gtk::CellRendererPixbuf::new();
        TreeViewColumnExt::pack_start(&name_column, &image, false);
        TreeViewColumnExt::add_attribute(&name_column, &image, "surface", 0);
        let text = gtk::CellRendererText::new();
        text.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        TreeViewColumnExt::pack_start(&name_column, &text, true);
        TreeViewColumnExt::add_attribute(&name_column, &text, "text", 1);
        TreeViewColumnExt::add_attribute(&name_column, &text, "foreground", 4);
        TreeViewColumnExt::add_attribute(&name_column, &text, "weight", 5);
        tree.append_column(&name_column);
        let size_column = gtk::TreeViewColumn::new();
        size_column.set_title("大小");
        let text = gtk::CellRendererText::new();
        text.set_xalign(1.0);
        TreeViewColumnExt::pack_start(&size_column, &text, false);
        TreeViewColumnExt::add_attribute(&size_column, &text, "text", 2);
        tree.append_column(&size_column);
        let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.add(&tree);
        root.pack_start(&scroll, true, true, 0);
        let status = label("打开文件栏时连接 SFTP", "dim");
        status.set_line_wrap(true);
        root.pack_start(&status, false, false, 0);
        let queue_title = label("传输队列", "heading");
        root.pack_start(&queue_title, false, false, 4);
        let queue = gtk::ListBox::new();
        queue.set_selection_mode(gtk::SelectionMode::None);
        let queue_scroll =
            gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        queue_scroll.set_size_request(-1, 160);
        queue_scroll.set_max_content_height(220);
        queue_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        queue_scroll.add(&queue);
        root.pack_end(&queue_scroll, false, true, 0);
        let state_path = profiles_path.parent().unwrap().join("sftp-transfers");
        let pane = Rc::new(Self {
            root,
            window: window.clone(),
            profiles_path,
            state_path,
            profile: RefCell::new(None),
            browser: RefCell::new(None),
            receiver: RefCell::new(None),
            generation: Cell::new(0),
            loading: Cell::new(false),
            path: RefCell::new(PathBuf::from(".")),
            address,
            host,
            status,
            model,
            icons: file_icons(window.scale_factor()),
            tree,
            entries: RefCell::new(Vec::new()),
            tasks: RefCell::new(Vec::new()),
            queue,
            queue_title,
            restoring: Cell::new(true),
            creating: Cell::new(0),
            closed: Cell::new(false),
            wake,
            polling: Cell::new(false),
            poll_dirty: Cell::new(false),
        });
        let weak = Rc::downgrade(&pane);
        close.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.hide();
            }
        });
        let weak = Rc::downgrade(&pane);
        refresh.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.refresh();
            }
        });
        let weak = Rc::downgrade(&pane);
        up.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                let parent = p
                    .path
                    .borrow()
                    .parent()
                    .unwrap_or(Path::new("/"))
                    .to_path_buf();
                p.navigate(parent);
            }
        });
        let weak = Rc::downgrade(&pane);
        pane.address.connect_activate(move |entry| {
            if let Some(p) = weak.upgrade() {
                p.navigate(PathBuf::from(entry.text().as_str()));
            }
        });
        let weak = Rc::downgrade(&pane);
        pane.tree.connect_row_activated(move |_, _, _| {
            if let Some(p) = weak.upgrade() {
                if let Some(item) = p.selected() {
                    if item.directory || item.symlink {
                        let path = p.path.borrow().join(item.name);
                        p.navigate(path);
                    } else {
                        p.download();
                    }
                }
            }
        });
        let weak = Rc::downgrade(&pane);
        upload.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.upload();
            }
        });
        let weak = Rc::downgrade(&pane);
        download.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.download();
            }
        });
        let weak = Rc::downgrade(&pane);
        mkdir.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.edit(false);
            }
        });
        let weak = Rc::downgrade(&pane);
        rename.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.edit(true);
            }
        });
        let weak = Rc::downgrade(&pane);
        delete.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                p.delete();
            }
        });
        let weak = Rc::downgrade(&pane);
        wake_receiver.attach(None, move |_| {
            if let Some(p) = weak.upgrade() {
                if p.closed.get() {
                    return glib::ControlFlow::Break;
                }
                p.ensure_poll();
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        let state = pane.state_path.clone();
        let weak = Rc::downgrade(&pane);
        job(
            move || Transfer::restore(&state),
            move |result| {
                if let Some(p) = weak.upgrade() {
                    p.restoring.set(false);
                    match result {
                        Ok(tasks) => {
                            for task in tasks {
                                p.add_task(task, false);
                            }
                        }
                        Err(error) => p.status.set_text(&error),
                    }
                }
            },
        );
        pane
    }
    pub fn show(self: &Rc<Self>, profile: Profile) {
        if self.closed.get() {
            return;
        }
        let changed = self.profile.borrow().as_ref().is_none_or(|old| {
            old.id != profile.id
                || old.host != profile.host
                || old.username != profile.username
                || old.port != profile.port
        });
        if changed {
            self.browser.borrow_mut().take();
            self.receiver.borrow_mut().take();
            self.model.clear();
            self.entries.borrow_mut().clear();
            self.path.replace(PathBuf::from("."));
            self.address.set_text(".");
            self.profile.replace(Some(profile));
        }
        self.root.set_no_show_all(false);
        self.root.show_all();
        self.root.set_no_show_all(true);
        self.root.show();
        self.connect();
    }
    pub fn hide(&self) {
        self.root.hide();
        self.browser.borrow_mut().take();
        self.receiver.borrow_mut().take();
        self.loading.set(false);
    }
    pub fn stop(&self) {
        self.closed.set(true);
        self.hide();
        for task in self.tasks.borrow_mut().iter_mut() {
            if let Some(control) = &task.control {
                control.stop();
            }
            task.receiver.take();
        }
    }
    pub fn running(&self) -> usize {
        self.tasks
            .borrow()
            .iter()
            .filter(|task| task.state == State::Running || task.state == State::Queued)
            .count()
    }
    fn connect(self: &Rc<Self>) {
        if self.browser.borrow().is_some() {
            return;
        }
        let Some(profile) = self.profile.borrow().clone() else {
            return;
        };
        self.host.set_text(&format!("文件 · {}", profile.name));
        self.status.set_text("正在连接 SFTP…");
        let (sender, receiver) = mpsc::sync_channel(8);
        let wake = self.wake.clone();
        self.receiver.replace(Some(receiver));
        self.browser.replace(Some(Browser::start(
            profile,
            self.profiles_path.clone(),
            move |event| {
                let sent = sender.send(event).is_ok();
                let _ = wake.send(());
                sent
            },
        )));
    }
    fn refresh(self: &Rc<Self>) {
        if self.browser.borrow().is_none() {
            self.connect();
        } else {
            let path = self.path.borrow().clone();
            self.navigate(path);
        }
    }
    fn navigate(&self, path: PathBuf) {
        if self.loading.get() {
            self.status.set_text("目录仍在读取，可收起文件栏停止连接");
            return;
        }
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        if self.request(BrowseRequest::List { generation, path }) {
            self.loading.set(true);
            self.model.clear();
            self.entries.borrow_mut().clear();
            self.status.set_text("正在读取目录…");
        }
    }
    fn request(&self, request: BrowseRequest) -> bool {
        let result = self
            .browser
            .borrow()
            .as_ref()
            .ok_or_else(|| "请先重新连接文件栏".to_string())
            .and_then(|browser| browser.request(request));
        if let Err(error) = result {
            self.status.set_text(&error);
            false
        } else {
            true
        }
    }
    fn selected(&self) -> Option<Entry> {
        if self.loading.get() {
            return None;
        }
        let (model, iter) = self.tree.selection().selected()?;
        let index = model.value(&iter, 3).get::<u32>().ok()? as usize;
        self.entries.borrow().get(index).cloned()
    }
    fn ensure_poll(self: &Rc<Self>) {
        self.poll_dirty.set(true);
        if self.closed.get() || self.polling.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let Some(p) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !p.closed.get() {
                p.poll_dirty.set(false);
                p.poll();
            }
            // Directory batches are drained gradually. Transfers report at 5 Hz
            // and wake this source themselves; an idle file pane has no timer.
            if !p.closed.get() && (p.loading.get() || p.poll_dirty.get()) {
                glib::ControlFlow::Continue
            } else {
                p.polling.set(false);
                glib::ControlFlow::Break
            }
        });
    }
    fn poll(self: &Rc<Self>) {
        // At most one 128-entry batch per tick; TreeView draws visible rows only.
        let event = self
            .receiver
            .borrow()
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok());
        if let Some(event) = event {
            match event {
                BrowserEvent::Ready => {
                    let path = self.path.borrow().clone();
                    self.navigate(path);
                }
                BrowserEvent::Entries(generation, entries)
                    if generation == self.generation.get() =>
                {
                    let mut stored = self.entries.borrow_mut();
                    for entry in entries {
                        let index = stored.len() as u32;
                        let kind = FileKind::of(&entry);
                        let size = entry
                            .size
                            .filter(|_| !entry.directory)
                            .map(size)
                            .unwrap_or_default();
                        self.model.insert_with_values(
                            None,
                            &[
                                (0, &self.icons[kind as usize]),
                                (1, &entry.name.to_string_lossy().as_ref()),
                                (2, &size),
                                (3, &index),
                                (4, &kind.name_color()),
                                (5, &kind.name_weight()),
                            ],
                        );
                        stored.push(entry);
                    }
                    self.status
                        .set_text(&format!("正在读取 · {} 项", stored.len()));
                }
                BrowserEvent::Listed(generation, result) if generation == self.generation.get() => {
                    self.loading.set(false);
                    match result {
                        Ok(path) => {
                            self.address.set_text(&path.to_string_lossy());
                            self.path.replace(path);
                            self.status.set_text(&format!(
                                "{} 项 · 双击目录进入，双击文件下载",
                                self.entries.borrow().len()
                            ));
                        }
                        Err(error) => {
                            self.address.set_text(&self.path.borrow().to_string_lossy());
                            self.status.set_text(&error);
                        }
                    }
                }
                BrowserEvent::Changed(result) => match result {
                    Ok(()) => self.refresh(),
                    Err(error) => self.status.set_text(&error),
                },
                BrowserEvent::Auth {
                    id,
                    prompt,
                    confirm,
                } => {
                    let auth = self
                        .browser
                        .borrow()
                        .as_ref()
                        .and_then(|browser| browser.control.authentication());
                    self.authenticate(auth, id, prompt, confirm);
                }
                BrowserEvent::Closed(error) => {
                    self.browser.borrow_mut().take();
                    self.receiver.borrow_mut().take();
                    self.loading.set(false);
                    self.status.set_text(&error);
                }
                _ => {}
            }
        }
        let mut events = Vec::new();
        {
            let tasks = self.tasks.borrow();
            for task in tasks.iter() {
                if let Some(receiver) = &task.receiver {
                    for _ in 0..8 {
                        match receiver.try_recv() {
                            Ok(event) => events.push((task.transfer.id.clone(), event)),
                            Err(_) => break,
                        }
                    }
                }
            }
        }
        for (id, event) in events {
            self.transfer_event(&id, event);
        }
        self.schedule();
    }
    fn authenticate(
        &self,
        auth: Option<SubsystemAuthentication>,
        id: String,
        prompt: String,
        confirm: bool,
    ) {
        let Some(auth) = auth else {
            return;
        };
        let dialog = gtk::Dialog::with_buttons(
            Some(if confirm {
                "SFTP · 确认主机指纹"
            } else {
                "SFTP · 身份认证"
            }),
            Some(&self.window),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            &[
                ("取消", gtk::ResponseType::Cancel),
                ("确定", gtk::ResponseType::Accept),
            ],
        );
        let content = dialog.content_area();
        content.style_context().add_class("dialog-content");
        content.set_spacing(12);
        let text = label(&prompt, "");
        text.set_line_wrap(true);
        text.set_selectable(true);
        text.set_max_width_chars(70);
        content.add(&text);
        let password = entry("输入认证口令", 4096);
        password.set_visibility(false);
        password.set_activates_default(true);
        if !confirm {
            content.add(&password);
        }
        dialog.set_default_response(gtk::ResponseType::Accept);
        dialog.show_all();
        let alive = dialog_guard(&dialog);
        let accepted = dialog.run() == gtk::ResponseType::Accept;
        let answer = accepted.then(|| {
            if confirm {
                "yes".into()
            } else {
                password.text().to_string()
            }
        });
        password.set_text("");
        dismiss(&dialog, &alive);
        let _ = auth.respond(&id, answer);
    }
    fn confirm(&self, title: &str, detail: &str) -> bool {
        let dialog = gtk::MessageDialog::new(
            Some(&self.window),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            gtk::MessageType::Question,
            gtk::ButtonsType::None,
            title,
        );
        dialog.set_secondary_text(Some(detail));
        dialog.add_buttons(&[
            ("取消", gtk::ResponseType::Cancel),
            ("确定", gtk::ResponseType::Accept),
        ]);
        let alive = dialog_guard(&dialog);
        let accepted = dialog.run() == gtk::ResponseType::Accept;
        dismiss(&dialog, &alive);
        accepted
    }
    fn edit(&self, rename: bool) {
        if self.loading.get() {
            return;
        }
        let selected = if rename { self.selected() } else { None };
        if rename && selected.is_none() {
            self.status.set_text("请选择要重命名的文件或目录");
            return;
        }
        let dialog = gtk::Dialog::with_buttons(
            Some(if rename { "重命名" } else { "新建目录" }),
            Some(&self.window),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            &[
                ("取消", gtk::ResponseType::Cancel),
                ("确定", gtk::ResponseType::Accept),
            ],
        );
        let name = entry("名称", 255);
        if rename {
            let warning = label(
                "目标名称须未被占用；若其他程序同时创建同名文件，服务器可能替换它。",
                "dim",
            );
            warning.set_line_wrap(true);
            warning.set_max_width_chars(48);
            dialog.content_area().add(&warning);
        }
        name.set_activates_default(true);
        if let Some(item) = &selected {
            name.set_text(&item.name.to_string_lossy());
        }
        dialog
            .content_area()
            .style_context()
            .add_class("dialog-content");
        dialog.content_area().add(&name);
        dialog.set_default_response(gtk::ResponseType::Accept);
        dialog.show_all();
        let alive = dialog_guard(&dialog);
        let accepted = dialog.run() == gtk::ResponseType::Accept;
        let name = name.text().to_string();
        dismiss(&dialog, &alive);
        if !accepted {
            return;
        }
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
            self.status.set_text("请输入不含路径分隔符的有效名称");
            return;
        }
        let to = self.path.borrow().join(&name);
        self.request(if let Some(item) = selected {
            BrowseRequest::Rename(self.path.borrow().join(item.name), to)
        } else {
            BrowseRequest::Mkdir(to)
        });
    }
    fn delete(&self) {
        let Some(item) = self.selected() else {
            self.status.set_text("请选择要删除的文件或空目录");
            return;
        };
        if self.confirm(
            "删除所选项目？",
            &format!(
                "{}\n远程删除不会进入回收站；非空目录不会递归删除。",
                item.name.to_string_lossy()
            ),
        ) {
            self.request(BrowseRequest::Remove(
                self.path.borrow().join(item.name),
                item.directory && !item.symlink,
            ));
        }
    }
    fn upload(self: &Rc<Self>) {
        if self.loading.get() || !self.path.borrow().is_absolute() {
            return;
        }
        let chooser = gtk::FileChooserDialog::new(
            Some("上传文件"),
            Some(&self.window),
            gtk::FileChooserAction::Open,
        );
        chooser.add_buttons(&[
            ("取消", gtk::ResponseType::Cancel),
            ("上传", gtk::ResponseType::Accept),
        ]);
        chooser.set_select_multiple(true);
        let alive = dialog_guard(&chooser);
        let accepted = chooser.run() == gtk::ResponseType::Accept;
        let files = chooser.filenames();
        dismiss(&chooser, &alive);
        if !accepted {
            return;
        }
        let mut selected = Vec::new();
        for local in files {
            let Some(name) = local.file_name() else {
                continue;
            };
            let remote = self.path.borrow().join(name);
            let exists = self
                .entries
                .borrow()
                .iter()
                .any(|entry| entry.name.as_os_str() == name);
            let replace = exists
                && self.confirm(
                    "替换远程同名文件？",
                    &format!("{}\n传输完成后才替换原文件。", name.to_string_lossy()),
                );
            if !exists || replace {
                selected.push((Direction::Upload, local, remote, replace));
            }
        }
        self.enqueue(selected);
    }
    fn download(self: &Rc<Self>) {
        let Some(item) = self.selected() else {
            self.status.set_text("请选择文件");
            return;
        };
        if item.directory {
            self.status.set_text("请选择普通文件下载");
            return;
        }
        let chooser = gtk::FileChooserDialog::new(
            Some("下载文件"),
            Some(&self.window),
            gtk::FileChooserAction::Save,
        );
        chooser.add_buttons(&[
            ("取消", gtk::ResponseType::Cancel),
            ("下载", gtk::ResponseType::Accept),
        ]);
        chooser.set_do_overwrite_confirmation(true);
        chooser.set_current_name(&item.name.to_string_lossy());
        let alive = dialog_guard(&chooser);
        let accepted = chooser.run() == gtk::ResponseType::Accept;
        let local = chooser.filename();
        dismiss(&chooser, &alive);
        if accepted {
            if let Some(local) = local {
                let replace = local.exists();
                self.enqueue(vec![(
                    Direction::Download,
                    local,
                    self.path.borrow().join(item.name),
                    replace,
                )]);
            }
        }
    }
    fn enqueue(self: &Rc<Self>, files: Vec<(Direction, PathBuf, PathBuf, bool)>) {
        if self.restoring.get() {
            self.status.set_text("正在恢复传输记录，请稍后");
            return;
        }
        if self.tasks.borrow().len() + self.creating.get() + files.len() > 128 {
            self.status
                .set_text("传输队列最多 128 条，请先移除已完成任务");
            return;
        }
        let Some(profile) = self.profile.borrow().clone() else {
            return;
        };
        let state = self.state_path.clone();
        let creating = files.len();
        self.creating.set(self.creating.get() + creating);
        let weak = Rc::downgrade(self);
        job(
            move || {
                let mut tasks: Vec<Transfer> = Vec::new();
                for (direction, local, remote, replace) in files {
                    let mut task =
                        match Transfer::new(direction, profile.clone(), local, remote, &state) {
                            Ok(task) => task,
                            Err(error) => {
                                for task in tasks {
                                    let _ = task.forget();
                                }
                                return Err(error);
                            }
                        };
                    if replace {
                        if let Err(error) = task.allow_replace() {
                            let _ = task.forget();
                            for task in tasks {
                                let _ = task.forget();
                            }
                            return Err(error);
                        }
                    }
                    tasks.push(task);
                }
                Ok(tasks)
            },
            move |result| {
                if let Some(p) = weak.upgrade() {
                    p.creating.set(p.creating.get().saturating_sub(creating));
                    match result {
                        Ok(tasks) => {
                            for task in tasks {
                                p.add_task(task, true);
                            }
                        }
                        Err(error) => p.status.set_text(&error),
                    }
                }
            },
        );
    }
    fn add_task(self: &Rc<Self>, transfer: Transfer, start: bool) {
        let row = gtk::ListBoxRow::new();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
        body.set_margin_top(6);
        body.set_margin_bottom(6);
        row.add(&body);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let filename = transfer
            .local
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let title = label(
            &format!(
                "{} · {filename}",
                if transfer.direction == Direction::Upload {
                    "上传"
                } else {
                    "下载"
                }
            ),
            "",
        );
        title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        title.set_tooltip_text(Some(&format!(
            "{}\n{}",
            transfer.profile.name,
            transfer.remote.to_string_lossy()
        )));
        heading.pack_start(&title, true, true, 0);
        let action = gtk::Button::with_label(if start { "暂停" } else { "继续" });
        let remove = icon_button("window-close-symbolic", "移除任务（未完成时保留临时文件）");
        remove.set_sensitive(!start);
        heading.pack_end(&remove, false, false, 0);
        heading.pack_end(&action, false, false, 0);
        body.pack_start(&heading, false, false, 0);
        let progress = gtk::ProgressBar::new();
        if transfer.total > 0 {
            progress.set_fraction(transfer.done as f64 / transfer.total as f64);
        }
        body.pack_start(&progress, false, false, 0);
        let detail = label(
            if start {
                "等待传输"
            } else {
                "已保留断点 · 点击继续恢复"
            },
            "dim",
        );
        detail.set_line_wrap(true);
        body.pack_start(&detail, false, false, 0);
        let id = transfer.id.clone();
        let weak = Rc::downgrade(self);
        action.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                let mut tasks = p.tasks.borrow_mut();
                if let Some(task) = tasks.iter_mut().find(|task| task.transfer.id == id) {
                    match task.state {
                        State::Running => {
                            if let Some(control) = &task.control {
                                control.stop();
                            }
                            task.detail.set_text("正在暂停…");
                            task.action.set_sensitive(false);
                        }
                        State::Queued => {
                            task.state = State::Paused;
                            task.action.set_label("继续");
                            task.detail.set_text("已暂停，可继续传输");
                        }
                        State::Paused => {
                            task.state = State::Queued;
                            task.action.set_label("暂停");
                            task.detail.set_text("等待传输");
                        }
                        State::Complete => {}
                    }
                }
                drop(tasks);
                p.ensure_poll();
            }
        });
        let id = transfer.id.clone();
        let weak = Rc::downgrade(self);
        remove.connect_clicked(move |_| {
            if let Some(p) = weak.upgrade() {
                let transfer = p.tasks.borrow().iter()
                    .find(|t| t.transfer.id == id && t.state != State::Running)
                    .map(|t| (t.transfer.clone(), t.state == State::Complete));
                let Some((transfer, complete)) = transfer else { return; };
                if !complete && !p.confirm("移除未完成传输？", &format!("断点记录将移除，临时文件保留以免误删数据：\n{}\n可在确认无需保留后自行删除临时文件。", transfer.partial_path().display())) { return; }
                let weak = Rc::downgrade(&p);
                let task_id = id.clone();
                job(move || transfer.forget(), move |result| {
                    if let Some(p) = weak.upgrade() {
                        if let Err(error) = result { p.status.set_text(&error); return; }
                        let mut tasks = p.tasks.borrow_mut();
                        if let Some(index) = tasks.iter().position(|t| t.transfer.id == task_id) {
                            let task = tasks.remove(index); p.queue.remove(&task.row);
                        }
                    }
                });
            }
        });
        row.show_all();
        self.queue.add(&row);
        self.tasks.borrow_mut().push(Task {
            transfer,
            state: if start { State::Queued } else { State::Paused },
            control: None,
            receiver: None,
            row,
            detail,
            progress,
            action,
            remove,
            last: (0, Instant::now()),
        });
        self.ensure_poll();
    }
    fn schedule(&self) {
        if self.closed.get() {
            return;
        }
        let mut tasks = self.tasks.borrow_mut();
        let mut running = tasks
            .iter()
            .filter(|task| task.state == State::Running)
            .count();
        for task in tasks.iter_mut() {
            if running >= 2 {
                break;
            }
            if task.state != State::Queued {
                continue;
            }
            let (sender, receiver) = mpsc::sync_channel(8);
            let wake = self.wake.clone();
            task.receiver = Some(receiver);
            task.control = Some(hshell_sftp::start_transfer(
                task.transfer.clone(),
                self.profiles_path.clone(),
                move |event| {
                    let sent = sender.send(event).is_ok();
                    let _ = wake.send(());
                    sent
                },
            ));
            task.state = State::Running;
            task.last = (task.transfer.done, Instant::now());
            task.detail.set_text("正在连接…");
            task.action.set_label("暂停");
            task.remove.set_sensitive(false);
            running += 1;
        }
        let pending = tasks
            .iter()
            .filter(|task| task.state == State::Queued)
            .count();
        let text = format!("传输 · {running} 进行中 · {pending} 等待");
        if self.queue_title.text() != text {
            self.queue_title.set_text(&text);
        }
    }
    fn transfer_event(self: &Rc<Self>, id: &str, event: TransferEvent) {
        if let TransferEvent::Auth {
            id: request,
            prompt,
            confirm,
        } = event
        {
            let auth = self
                .tasks
                .borrow()
                .iter()
                .find(|task| task.transfer.id == id)
                .and_then(|task| task.control.as_ref())
                .and_then(WorkerControl::authentication);
            self.authenticate(auth, request, prompt, confirm);
            return;
        }
        let mut tasks = self.tasks.borrow_mut();
        let Some(task) = tasks.iter_mut().find(|task| task.transfer.id == id) else {
            return;
        };
        let mut refresh = false;
        match event {
            TransferEvent::Progress(progress) => {
                task.progress.set_fraction(if progress.total == 0 {
                    0.0
                } else {
                    progress.bytes as f64 / progress.total as f64
                });
                let speed = if progress.verifying {
                    String::new()
                } else {
                    let elapsed = task.last.1.elapsed().as_secs_f64().max(0.001);
                    let value = progress.bytes.saturating_sub(task.last.0) as f64 / elapsed;
                    task.last = (progress.bytes, Instant::now());
                    format!(" · {}/s", size(value as u64))
                };
                task.detail.set_text(&format!(
                    "{}{} / {}{}",
                    if progress.verifying {
                        "校验断点 · "
                    } else {
                        ""
                    },
                    size(progress.bytes),
                    size(progress.total),
                    speed
                ));
            }
            TransferEvent::Finished(transfer, result) => {
                task.transfer = *transfer;
                task.control.take();
                task.receiver.take();
                task.action.set_sensitive(true);
                match result {
                    Ok(()) => {
                        refresh = task.transfer.direction == Direction::Upload
                            && self.root.is_visible()
                            && self
                                .profile
                                .borrow()
                                .as_ref()
                                .is_some_and(|p| p.id == task.transfer.profile.id);
                        task.state = State::Complete;
                        task.progress.set_fraction(1.0);
                        task.detail.set_text("传输完成");
                        task.action.set_sensitive(false);
                        task.remove.set_sensitive(true);
                    }
                    Err(error) => {
                        task.state = State::Paused;
                        task.detail.set_text(&error);
                        task.action.set_label("继续");
                        task.remove.set_sensitive(true);
                    }
                }
            }
            _ => {}
        }
        drop(tasks);
        if refresh && !self.loading.get() {
            self.refresh();
        }
    }
}

fn size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(feature = "native-probe")]
async fn until_checkpoint(pane: &FilePane) -> Result<(), String> {
    let id = pane
        .tasks
        .borrow()
        .last()
        .ok_or("No transfer")?
        .transfer
        .id
        .clone();
    // A running worker can still be connecting. Interrupt only after the real
    // durable journal records a partial transfer, independent of SSH startup.
    crate::probe::until(
        || {
            Transfer::restore(&pane.state_path).is_ok_and(|tasks| {
                tasks
                    .iter()
                    .any(|task| task.id == id && task.done > 0 && task.done < task.total)
            })
        },
        "Interrupted transfer did not persist a partial checkpoint",
    )
    .await
}

#[cfg(feature = "native-probe")]
pub(crate) async fn verify(client: &Rc<crate::ui::Client>, result: &Path) -> Result<(), String> {
    use crate::{
        probe::{pause, snapshot, until, until_with_timeout},
        probe_events::{key, scroll},
    };
    use gtk::gdk;
    let root = PathBuf::from(std::env::var_os("HSHELL_SFTP_ROOT").ok_or("Missing fixture root")?);
    let profile = Profile {
        id: "gui-sftp".into(),
        name: "开发服务器".into(),
        host: "fixture".into(),
        username: String::new(),
        port: None,
        identity_file: String::new(),
        group: String::new(),
        favorite: false,
        password_saved: false,
    };
    client.connect_profile(profile.clone());
    until(
        || {
            client
                .active()
                .is_some_and(|t| t.phase.get() == crate::ui::Phase::Running)
        },
        "Terminal did not start",
    )
    .await?;
    let tab = client.active().ok_or("No terminal")?;
    tab.terminal.send("seq 1 5000\n");
    until(
        || tab.terminal.recent_text(100).contains("5000"),
        "Terminal did not fill",
    )
    .await?;
    std::fs::write(result, r#"{"phase":"terminal-baseline"}"#).map_err(|e| e.to_string())?;
    pause(500).await;
    std::fs::write(result, r#"{"phase":"directory"}"#).map_err(|e| e.to_string())?;
    let pane = client.open_files(profile.clone());
    until(
        || !pane.restoring.get() && pane.path.borrow().is_absolute() && !pane.loading.get(),
        "SFTP browser did not connect",
    )
    .await?;
    if std::env::var_os("HSHELL_NATIVE_SFTP_APPEARANCE").is_some() {
        pane.navigate(PathBuf::from("/fixture/data/appearance"));
        until(|| !pane.loading.get(), "Appearance directory did not load").await?;
        if pane.entries.borrow().len() != 8 || pane.icons.iter().any(Option::is_none) {
            return Err(format!(
                "Appearance entries: {}; missing icons: {:?}",
                pane.entries.borrow().len(),
                FileKind::ALL
                    .iter()
                    .zip(&pane.icons)
                    .filter(|(_, icon)| icon.is_none())
                    .map(|(kind, _)| kind.icon_name())
                    .collect::<Vec<_>>()
            ));
        }
        let palette:Vec<_> = pane.entries.borrow().iter().map(|entry| {
            let kind=FileKind::of(entry);
            serde_json::json!({"name":entry.name.to_string_lossy(),"icon":kind.icon_name(),"iconColor":kind.icon_color(),"nameColor":kind.name_color()})
        }).collect();
        tab.terminal
            .send("clear\nprintf 'HShell SFTP workspace\\n\\n'\n");
        pause(250).await;
        pane.tree.selection().unselect_all();
        snapshot(client, "files")?;
        let index = pane
            .entries
            .borrow()
            .iter()
            .position(|entry| entry.directory)
            .ok_or("No folder")?;
        pane.tree
            .selection()
            .select_path(&gtk::TreePath::from_indicesv(&[index as i32]));
        pane.tree.grab_focus();
        pause(100).await;
        snapshot(client, "files-selected")?;
        client.window.resize(800, 580);
        pause(500).await;
        snapshot(client, "files-small")?;
        return std::fs::write(result,serde_json::json!({"phase":"complete","entries":palette,"iconCacheEntries":pane.icons.len(),"scaleFactor":client.window.scale_factor(),"selectionChecked":true}).to_string()).map_err(|e|e.to_string());
    }
    if std::env::var_os("HSHELL_NATIVE_SFTP_LAYOUT").is_some() {
        client.window.resize(800, 580);
        pause(1000).await;
        snapshot(client, "layout")?;
        return Err(client.file_layout().to_string());
    }
    if std::env::var_os("HSHELL_NATIVE_SFTP_RESTORE").is_some() {
        until(|| !pane.tasks.borrow().is_empty(), "No restored transfer").await?;
        let action = pane.tasks.borrow().last().unwrap().action.clone();
        let done = pane.tasks.borrow().last().unwrap().transfer.done;
        if pane.tasks.borrow().last().unwrap().state != State::Paused || done == 0 {
            return Err(format!(
                "Transfer did not restore as paused with checkpoint: paused={}, done={done}, total={}",
                pane.tasks.borrow().last().unwrap().state == State::Paused,
                pane.tasks.borrow().last().unwrap().transfer.total
            ));
        }
        action.emit_clicked();
        until_with_timeout(
            || pane.tasks.borrow().last().unwrap().state == State::Complete,
            "Restored GUI transfer did not finish",
            Duration::from_secs(60),
        )
        .await?;
        return std::fs::write(
            result,
            serde_json::json!({"phase":"complete","restoredAfterExit":true,"checkpoint":done})
                .to_string(),
        )
        .map_err(|e| e.to_string());
    }
    pane.navigate(PathBuf::from("/fixture/data/large"));
    let mut directory_ticks = Vec::new();
    while pane.loading.get() {
        let before = Instant::now();
        pause(16).await;
        directory_ticks.push(before.elapsed().as_secs_f64() * 1000.0);
        if directory_ticks.len() > 1500 {
            return Err("Large directory stalled".into());
        }
    }
    if pane.entries.borrow().len() != 10000 {
        return Err(format!(
            "Large directory incomplete: {}",
            pane.status.text()
        ));
    }
    pane.navigate(PathBuf::from("/fixture/data"));
    until(|| !pane.loading.get(), "Directory navigation stalled").await?;
    let upload = root.join("gui-upload.local");
    let remote_source = root.join("data/gui-source");
    for path in [&upload, &remote_source] {
        std::fs::File::create(path)
            .and_then(|f| f.set_len(512 * 1024 * 1024))
            .map_err(|e| e.to_string())?;
    }
    pane.enqueue(vec![
        (
            Direction::Upload,
            upload,
            PathBuf::from("/fixture/data/gui-upload"),
            false,
        ),
        (
            Direction::Download,
            root.join("gui-download.local"),
            PathBuf::from("/fixture/data/gui-source"),
            false,
        ),
    ]);
    until(|| pane.running() == 2, "Two transfers did not start").await?;
    std::fs::write(result, r#"{"phase":"transferring"}"#).map_err(|e| e.to_string())?;
    let mut input = Vec::new();
    let mut scroll_ticks = Vec::new();
    let mut simultaneous = 0;
    tab.terminal.widget.grab_focus();
    for index in 0..60 {
        let running = pane.running();
        if running == 2 {
            simultaneous += 1;
        }
        if running > 0 && index % 3 == 0 {
            let marker = format!("hsftp{index:02}");
            let started = Instant::now();
            for character in marker.chars() {
                key(
                    &tab.terminal.widget,
                    character as u32,
                    gdk::ModifierType::empty(),
                )?;
            }
            until(
                || tab.terminal.recent_text(50).contains(&marker),
                "Keyboard echo stalled during SFTP",
            )
            .await?;
            input.push(started.elapsed().as_secs_f64() * 1000.0);
            key(&tab.terminal.widget, 0xff0d, gdk::ModifierType::empty())?;
        }
        let started = Instant::now();
        scroll(&tab.terminal.widget, index % 2 == 0)?;
        pause(16).await;
        if running > 0 {
            scroll_ticks.push(started.elapsed().as_secs_f64() * 1000.0);
        }
    }
    if input.len() < 3 || simultaneous < 3 {
        return Err("Insufficient simultaneous transfer samples".into());
    }
    // Bulk completion depends on the runner's disk and SSH throughput. Keep
    // the separate 100/50 ms input/scroll budgets unchanged below.
    until_with_timeout(
        || pane.running() == 0,
        "Transfers did not finish",
        Duration::from_secs(60),
    )
    .await?;
    if pane
        .tasks
        .borrow()
        .iter()
        .any(|t| t.state != State::Complete)
    {
        return Err(format!(
            "Transfer failed: {:?}",
            pane.tasks
                .borrow()
                .iter()
                .map(|t| t.detail.text())
                .collect::<Vec<_>>()
        ));
    }
    snapshot(client, "sftp")?;
    // A paused transfer survives hiding/reopening the pane, and the stop button
    // is tested through its GTK signal rather than a separate transport mock.
    pane.enqueue(vec![(
        Direction::Upload,
        root.join("gui-upload.local"),
        PathBuf::from("/fixture/data/gui-pause"),
        false,
    )]);
    until(
        || {
            pane.tasks
                .borrow()
                .last()
                .is_some_and(|t| t.state == State::Running)
        },
        "Pause fixture did not start",
    )
    .await?;
    until_checkpoint(&pane).await?;
    let action = pane.tasks.borrow().last().unwrap().action.clone();
    action.emit_clicked();
    until(
        || pane.tasks.borrow().last().unwrap().state == State::Paused,
        "Pause did not settle",
    )
    .await?;
    pane.hide();
    pause(100).await;
    pane.show(profile);
    action.emit_clicked();
    until_with_timeout(
        || pane.tasks.borrow().last().unwrap().state == State::Complete,
        "Resume did not finish",
        Duration::from_secs(60),
    )
    .await?;
    client.window.resize(800, 580);
    pane.hide();
    client.open_files(pane.profile.borrow().clone().unwrap());
    pause(500).await;
    snapshot(client, "sftp-small")?;
    if tab.terminal.widget.allocated_width() < 300 {
        return Err(format!(
            "Minimum window leaves terminal too narrow: terminal {}, window {}, file pane {}",
            tab.terminal.widget.allocated_width(),
            client.window.allocated_width(),
            pane.root.allocated_width()
        ));
    }
    let terminal_width = tab.terminal.widget.allocated_width();
    std::fs::write(result, r#"{"phase":"hidden-idle"}"#).map_err(|e| e.to_string())?;
    pane.hide();
    pause(500).await;
    if pane.polling.get() {
        return Err("Hidden idle file pane retains polling timer".into());
    }
    input.sort_by(f64::total_cmp);
    scroll_ticks.sort_by(f64::total_cmp);
    directory_ticks.sort_by(f64::total_cmp);
    let p95 = |values: &[f64]| values[((values.len() - 1) as f64 * 0.95) as usize];
    if p95(&input) > 100.0 || p95(&scroll_ticks) > 50.0 || p95(&directory_ticks) > 50.0 {
        return Err(format!(
            "UI budget exceeded: input {}, scroll {}, directory {}",
            p95(&input),
            p95(&scroll_ticks),
            p95(&directory_ticks)
        ));
    }
    pane.enqueue(vec![(
        Direction::Upload,
        root.join("gui-upload.local"),
        PathBuf::from("/fixture/data/gui-exit"),
        false,
    )]);
    until(|| pane.running() == 1, "Exit fixture did not start").await?;
    until_checkpoint(&pane).await?;
    let answered = crate::probe::answer_confirmation(gtk::ResponseType::Cancel, None);
    client.window.close();
    until(|| answered.get(), "Close cancellation not shown").await?;
    if pane.closed.get() {
        return Err("Cancelled close stopped SFTP".into());
    }
    let answered =
        crate::probe::answer_confirmation(gtk::ResponseType::Accept, Some(client.window.clone()));
    client.window.close();
    until(
        || answered.get() && pane.closed.get(),
        "Close during transfer did not finish",
    )
    .await?;
    let output = serde_json::json!({"phase":"complete","largeDirectory":10000,"directoryTickP95Ms":p95(&directory_ticks),"inputP95Ms":p95(&input),"inputSamples":input.len(),"scrollTickP95Ms":p95(&scroll_ticks),"simultaneousTransferTicks":simultaneous,"transferredMiB":1536,"pauseResume":true,"exitDuringTransfer":true,"hiddenIdleTimerStopped":true,"minimumTerminalWidth":terminal_width,"notes":"Synthetic GTK keyboard/scroll events to real SSH; echo ends at VTE parsing, scroll timing is main-loop scheduling, not physical display latency."});
    std::fs::write(result, output.to_string()).map_err(|e| e.to_string())
}
