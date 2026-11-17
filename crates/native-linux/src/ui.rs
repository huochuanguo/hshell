use crate::terminal::Terminal;
use gtk::{gdk, glib, prelude::*};
use ssh_core::{DirectSession, PasswordUpdate, Profile, SessionEvent};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, PartialEq)]
pub enum Phase {
    Starting,
    Running,
    Exited,
}

pub struct Tab {
    pub id: String,
    pub profile: Profile,
    pub terminal: Terminal,
    pub phase: Cell<Phase>,
    pub closed: Cell<bool>,
    pub session: RefCell<Option<DirectSession>>,
    root: gtk::Box,
    label: gtk::Label,
    banner: gtk::Label,
    pending_auth: RefCell<VecDeque<(String, String, bool)>>,
}

struct HostRow {
    widget: gtk::ListBoxRow,
    profile: Profile,
    name: gtk::Label,
    address: gtk::Label,
    favorite_button: gtk::Button,
    search: String,
    visible: bool,
}

struct HostGroup {
    name: String,
    header: gtk::ListBoxRow,
    rows: Vec<String>,
}

pub struct Client {
    pub window: gtk::ApplicationWindow,
    pub tabs: RefCell<Vec<Rc<Tab>>>,
    pub notebook: gtk::Notebook,
    pub(crate) profiles: RefCell<Vec<Profile>>,
    profiles_path: PathBuf,
    config_lock: Arc<Mutex<()>>,
    profile_cache: Arc<Mutex<ssh_core::ProfileCache>>,
    writable: Cell<bool>,
    saving: Cell<bool>,
    closing: Cell<bool>,
    confirming_close: Cell<bool>,
    reclaim_source: RefCell<Option<glib::SourceId>>,
    sidebar: gtk::Box,
    host_pane: gtk::Paned,
    hosts: gtk::ListBox,
    host_groups: RefCell<Vec<HostGroup>>,
    host_rows: RefCell<HashMap<String, HostRow>>,
    host_order: RefCell<HashMap<String, i32>>,
    pending_hosts: RefCell<VecDeque<Profile>>,
    host_source: RefCell<Option<glib::SourceId>>,
    host_search: gtk::SearchEntry,
    favorites: gtk::ToggleButton,
    quick_host: gtk::Entry,
    quick_port: gtk::Entry,
    status: gtk::Label,
    search_row: gtk::Box,
    search_input: gtk::Entry,
    search_result: gtk::Label,
    session_address: gtk::Label,
    disconnect: gtk::Button,
    reconnect: gtk::Button,
    files: RefCell<Option<Rc<crate::files::FilePane>>>,
    terminal_pane: gtk::Paned,
}

pub(crate) fn icon(name: &str) -> gtk::Image {
    gtk::Image::from_icon_name(Some(name), gtk::IconSize::Button)
}
pub(crate) fn icon_button(name: &str, hint: &str) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_image(Some(&icon(name)));
    button.set_tooltip_text(Some(hint));
    button
}
pub(crate) fn label(text: &str, class: &str) -> gtk::Label {
    let item = gtk::Label::new(Some(text));
    item.set_xalign(0.0);
    if !class.is_empty() {
        item.style_context().add_class(class);
    }
    item
}
pub(crate) fn entry(placeholder: &str, max: i32) -> gtk::Entry {
    let item = gtk::Entry::new();
    item.set_placeholder_text(Some(placeholder));
    item.set_max_length(max);
    item
}
pub(crate) fn dialog_guard(dialog: &impl IsA<gtk::Widget>) -> Rc<Cell<bool>> {
    let alive = Rc::new(Cell::new(true));
    let on_destroy = alive.clone();
    dialog.connect_destroy(move |_| on_destroy.set(false));
    alive
}
pub(crate) fn dismiss(dialog: &impl IsA<gtk::Widget>, alive: &Cell<bool>) {
    if alive.replace(false) {
        unsafe {
            dialog.destroy();
        }
    }
}

#[allow(deprecated)]
pub fn job<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    done: impl FnOnce(Result<T, String>) + 'static,
) {
    let (sender, receiver) = glib::MainContext::channel(glib::Priority::DEFAULT);
    let mut done = Some(done);
    receiver.attach(None, move |result| {
        if let Some(done) = done.take() {
            done(result);
        }
        glib::ControlFlow::Break
    });
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
}

impl Client {
    pub fn new(app: &gtk::Application) -> Rc<Self> {
        load_fonts();
        #[cfg(feature = "native-probe")]
        crate::timing::mark("fonts-ready");
        if let Some(settings) = gtk::Settings::default() {
            settings.set_property("gtk-application-prefer-dark-theme", true);
        }
        let css = gtk::CssProvider::new();
        css.load_from_data(include_bytes!("style.css"))
            .expect("valid client CSS");
        if let Some(screen) = gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let window = gtk::ApplicationWindow::new(app);
        window.set_title("HShell");
        window.set_default_size(1280, 820);
        window.set_size_request(800, 580);
        let loader = gtk::gdk_pixbuf::PixbufLoader::new();
        let _ = loader.write(include_bytes!("../../../src-tauri/icons/128x128.png"));
        let _ = loader.close();
        window.set_icon(loader.pixbuf().as_ref());
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&root);
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        toolbar.style_context().add_class("toolbar");
        let brand_image = gtk::Image::from_pixbuf(
            loader
                .pixbuf()
                .and_then(|image| image.scale_simple(24, 24, gtk::gdk_pixbuf::InterpType::Bilinear))
                .as_ref(),
        );
        toolbar.pack_start(&brand_image, false, false, 0);
        toolbar.pack_start(&label("HShell", "heading"), false, false, 0);
        let new = gtk::Button::with_label("新建连接");
        new.set_image(Some(&icon("list-add-symbolic")));
        toolbar.pack_start(&new, false, false, 8);
        let toggle = gtk::Button::with_label("主机栏");
        toggle.set_tooltip_text(Some("显示 / 隐藏主机栏 · Ctrl Shift B"));
        toggle.set_image(Some(&icon("view-sidebar-symbolic")));
        toolbar.pack_start(&toggle, false, false, 0);
        let info = icon_button("help-about-symbolic", "客户端信息与快捷键");
        toolbar.pack_end(&info, false, false, 0);
        root.pack_start(&toolbar, false, false, 0);
        let pane = gtk::Paned::new(gtk::Orientation::Horizontal);
        pane.set_position(244);
        root.pack_start(&pane, true, true, 0);
        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 10);
        sidebar.style_context().add_class("sidebar");
        sidebar.set_size_request(195, -1);
        let sidebar_title = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        sidebar_title.pack_start(&label("主机", "heading"), true, true, 8);
        let add = icon_button("list-add-symbolic", "添加主机");
        sidebar_title.pack_end(&add, false, false, 0);
        sidebar.pack_start(&sidebar_title, false, false, 0);
        let host_search = gtk::SearchEntry::new();
        host_search.set_placeholder_text(Some("搜索主机…"));
        sidebar.pack_start(&host_search, false, false, 0);
        let favorites = gtk::ToggleButton::with_label("只看收藏");
        favorites.set_image(Some(&icon("starred-symbolic")));
        sidebar.pack_start(&favorites, false, false, 0);
        let hosts = gtk::ListBox::new();
        hosts.set_activate_on_single_click(false);
        hosts.set_selection_mode(gtk::SelectionMode::Single);
        let hosts_scroll =
            gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        hosts_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        hosts_scroll.add(&hosts);
        sidebar.pack_start(&hosts_scroll, true, true, 0);
        sidebar.pack_end(&label("双击主机连接 · Enter 打开", "dim"), false, false, 4);
        pane.pack1(&sidebar, false, true);
        let workspace = gtk::Box::new(gtk::Orientation::Vertical, 0);
        pane.pack2(&workspace, true, false);
        let quickbar = gtk::Box::new(gtk::Orientation::Horizontal, 9);
        quickbar.style_context().add_class("quickbar");
        quickbar.pack_start(&label("SSH", "dim"), false, false, 0);
        let quick_host = entry("用户名@主机地址，或 SSH 别名", 384);
        quick_host.set_tooltip_text(Some("快速连接 · Ctrl Shift K"));
        quick_host.set_hexpand(true);
        quickbar.pack_start(&quick_host, true, true, 0);
        let quick_port = entry("配置 / 22", 5);
        quick_port.set_width_chars(8);
        quick_port.set_input_purpose(gtk::InputPurpose::Digits);
        quickbar.pack_start(&quick_port, false, false, 0);
        let quick = gtk::Button::with_label("连接");
        quick.style_context().add_class("primary");
        quickbar.pack_start(&quick, false, false, 0);
        let quick_save = icon_button("bookmark-new-symbolic", "保存为主机");
        quickbar.pack_start(&quick_save, false, false, 0);
        workspace.pack_start(&quickbar, false, false, 0);
        let sessionbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        sessionbar.style_context().add_class("quickbar");
        let session_address = label("选择主机，或输入地址连接", "address");
        session_address.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        sessionbar.pack_start(&session_address, true, true, 0);
        let disconnect = gtk::Button::with_label("断开");
        let reconnect = gtk::Button::with_label("重连");
        sessionbar.pack_start(&disconnect, false, false, 0);
        sessionbar.pack_start(&reconnect, false, false, 0);
        let files = gtk::Button::with_label("文件");
        files.set_image(Some(&icon("folder-remote-symbolic")));
        files.set_tooltip_text(Some("打开 / 收起 SFTP 文件栏"));
        sessionbar.pack_start(&files, false, false, 0);
        let copy = icon_button("edit-copy-symbolic", "复制选中内容 · Ctrl Shift C");
        let find = icon_button("edit-find-symbolic", "搜索终端 · Ctrl Shift F");
        sessionbar.pack_start(&copy, false, false, 0);
        sessionbar.pack_start(&find, false, false, 0);
        workspace.pack_start(&sessionbar, false, false, 0);
        let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        search_row.style_context().add_class("quickbar");
        search_row.set_no_show_all(true);
        let search_input = entry("搜索当前终端", 1000);
        search_row.pack_start(&search_input, true, true, 0);
        let search_result = label("", "dim");
        search_row.pack_start(&search_result, false, false, 0);
        let prev = icon_button("go-up-symbolic", "上一个结果");
        let next = icon_button("go-down-symbolic", "下一个结果");
        let close_search = icon_button("window-close-symbolic", "关闭搜索");
        for button in [&prev, &next, &close_search] {
            search_row.pack_start(button, false, false, 0);
        }
        workspace.pack_start(&search_row, false, false, 0);
        let notebook = gtk::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_hexpand(true);
        notebook.set_vexpand(true);
        let terminal_pane = gtk::Paned::new(gtk::Orientation::Horizontal);
        terminal_pane.pack1(&notebook, true, false);
        workspace.pack_start(&terminal_pane, true, true, 0);
        let home = gtk::Box::new(gtk::Orientation::Vertical, 16);
        home.set_valign(gtk::Align::Center);
        home.set_halign(gtk::Align::Center);
        home.pack_start(&icon("utilities-terminal-symbolic"), false, false, 0);
        home.pack_start(&label("新建会话", "heading"), false, false, 0);
        home.pack_start(
            &label("输入服务器地址，或从左侧主机栏连接。", "dim"),
            false,
            false,
            0,
        );
        let home_new = gtk::Button::with_label("新建连接");
        home_new.style_context().add_class("primary");
        home.pack_start(&home_new, false, false, 0);
        notebook.append_page(&home, Some(&gtk::Label::new(Some("新建会话"))));
        let statusbar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        statusbar.style_context().add_class("statusbar");
        let status = label("正在读取连接…", "dim");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        statusbar.pack_start(&status, true, true, 0);
        statusbar.pack_end(&label("UTF-8  xterm-256color", "dim"), false, false, 0);
        root.pack_end(&statusbar, false, false, 0);
        let profiles_path = glib::user_config_dir().join("io.qizhan.ssh/profiles.json");
        // Keep the existing storage namespace so renaming never loses saved hosts.
        let client = Rc::new(Self {
            window,
            tabs: RefCell::new(Vec::new()),
            notebook,
            profiles: RefCell::new(Vec::new()),
            profiles_path,
            config_lock: Arc::new(Mutex::new(())),
            profile_cache: Arc::new(Mutex::new(ssh_core::ProfileCache::default())),
            writable: Cell::new(false),
            saving: Cell::new(false),
            closing: Cell::new(false),
            confirming_close: Cell::new(false),
            reclaim_source: RefCell::new(None),
            sidebar,
            host_pane: pane,
            hosts,
            host_groups: RefCell::new(Vec::new()),
            host_rows: RefCell::new(HashMap::new()),
            host_order: RefCell::new(HashMap::new()),
            pending_hosts: RefCell::new(VecDeque::new()),
            host_source: RefCell::new(None),
            host_search,
            favorites,
            quick_host,
            quick_port,
            status,
            search_row,
            search_input,
            search_result,
            session_address,
            disconnect,
            reconnect,
            files: RefCell::new(None),
            terminal_pane,
        });
        let weak = Rc::downgrade(&client);
        client.hosts.set_sort_func(Some(Box::new(move |a, b| {
            let Some(client) = weak.upgrade() else {
                return 0;
            };
            let order = client.host_order.borrow();
            order
                .get(a.widget_name().as_str())
                .cmp(&order.get(b.widget_name().as_str())) as i32
        })));
        for button in [&new, &add, &home_new] {
            Self::button(button, &client, |client| client.editor(None));
        }
        Self::button(&toggle, &client, |client| client.toggle_sidebar());
        Self::button(&info, &client, |client| client.info());
        Self::button(&quick, &client, |client| client.quick_connect());
        Self::button(&quick_save, &client, |client| {
            match client.quick_profile() {
                Ok(profile) => client.editor(Some(profile)),
                Err(error) => client.error(&error),
            }
        });
        Self::button(&copy, &client, |client| client.copy());
        Self::button(&find, &client, |client| client.show_search());
        let weak = Rc::downgrade(&client);
        files.connect_clicked(move |_| {
            let Some(client) = weak.upgrade() else {
                return;
            };
            if let Some(pane) = client.files.borrow().as_ref() {
                if pane.root.is_visible() {
                    pane.hide();
                    return;
                }
            }
            let Some(tab) = client.active() else {
                client.notice("请先打开一个服务器会话");
                return;
            };
            client.open_files(tab.profile.clone());
        });
        Self::button(&next, &client, |client| client.find(false));
        Self::button(&prev, &client, |client| client.find(true));
        Self::button(&close_search, &client, |client| {
            client.search_row.hide();
            if let Some(tab) = client.active() {
                tab.terminal.find("", false);
                tab.terminal.widget.grab_focus();
            }
        });
        Self::button(&client.disconnect, &client, |client| {
            if let Some(tab) = client.active() {
                if client.confirm("断开当前会话？", "SSH 连接将断开，终端输出会保留。")
                {
                    if let Some(session) = tab.session.borrow().as_ref() {
                        session.close();
                    }
                }
            }
        });
        Self::button(&client.reconnect, &client, |client| {
            if let Some(tab) = client.active() {
                let profile = tab.profile.clone();
                client.close_tab(&tab.id, false);
                client.connect_profile(profile);
            }
        });
        let weak = Rc::downgrade(&client);
        client.quick_host.connect_activate(move |_| {
            if let Some(client) = weak.upgrade() {
                client.quick_connect();
            }
        });
        let weak = Rc::downgrade(&client);
        client.search_input.connect_activate(move |_| {
            if let Some(client) = weak.upgrade() {
                client.find(false);
            }
        });
        let weak = Rc::downgrade(&client);
        client.host_search.connect_search_changed(move |_| {
            if let Some(client) = weak.upgrade() {
                client.filter_hosts();
            }
        });
        let weak = Rc::downgrade(&client);
        client.favorites.connect_toggled(move |_| {
            if let Some(client) = weak.upgrade() {
                client.filter_hosts();
            }
        });
        let weak = Rc::downgrade(&client);
        client.hosts.connect_row_activated(move |_, row| {
            if let Some(client) = weak.upgrade() {
                if let Some(id) = row.widget_name().strip_prefix("host-") {
                    let profile = client
                        .profiles
                        .borrow()
                        .iter()
                        .find(|profile| profile.id == id)
                        .cloned();
                    if let Some(profile) = profile {
                        client.connect_profile(profile);
                    }
                }
            }
        });
        let weak = Rc::downgrade(&client);
        client.notebook.connect_switch_page(move |_, _, _| {
            if let Some(client) = weak.upgrade() {
                let weak = Rc::downgrade(&client);
                glib::idle_add_local_once(move || {
                    if let Some(client) = weak.upgrade() {
                        client.update_session();
                        let pane = client.files.borrow().clone();
                        if let (Some(pane), Some(tab)) = (pane, client.active()) {
                            if pane.root.is_visible() {
                                pane.show(tab.profile.clone());
                            }
                        }
                    }
                });
            }
        });
        let weak = Rc::downgrade(&client);
        client.window.connect_size_allocate(move |_, allocation| {
            if let Some(client) = weak.upgrade() {
                if allocation.width() < 1050
                    && client.sidebar.is_visible()
                    && client
                        .files
                        .borrow()
                        .as_ref()
                        .is_some_and(|pane| pane.root.is_visible())
                {
                    // GTK ignores resize requests made during size-allocation.
                    // Collapse the sidebar after this allocation has returned.
                    let weak = Rc::downgrade(&client);
                    glib::idle_add_local_once(move || {
                        if let Some(client) = weak.upgrade() {
                            if client.window.allocated_width() < 1050 && client.sidebar.is_visible()
                            {
                                client.hide_sidebar();
                                client
                                    .terminal_pane
                                    .set_position((client.window.allocated_width() - 360).max(300));
                            }
                        }
                    });
                }
            }
        });
        let weak = Rc::downgrade(&client);
        client.window.connect_delete_event(move |_, _| {
            let Some(client) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if client.saving.get() {
                client.notice("正在保存连接，请稍后再退出。");
                return glib::Propagation::Stop;
            }
            if client.confirming_close.replace(true) {
                return glib::Propagation::Stop;
            }
            let transfers = client.files.borrow().as_ref().map_or(0, |files| files.running());
            if (transfers > 0 || client
                .tabs
                .borrow()
                .iter()
                .any(|tab| tab.phase.get() != Phase::Exited))
                && !client.confirm(
                    "退出HShell？",
                    "SSH 会话将断开，文件传输将暂停并保留断点，下次打开文件栏可继续。需要继续的远端任务请先放入 tmux 或 screen。",
                )
            {
                client.confirming_close.set(false);
                return glib::Propagation::Stop;
            }
            client.shutdown();
            glib::Propagation::Proceed
        });
        client.bind_keys();
        #[cfg(feature = "native-probe")]
        {
            crate::timing::mark("ui-ready");
            client.window.connect_draw(|_, _| {
                crate::timing::mark("first-draw-dispatch");
                glib::Propagation::Proceed
            });
        }
        client.window.show_all();
        #[cfg(feature = "native-probe")]
        crate::timing::mark("window-shown");
        client.search_row.hide();
        client.update_session();
        let path = client.profiles_path.clone();
        let cache = client.profile_cache.clone();
        let weak = Rc::downgrade(&client);
        job(
            move || {
                cache
                    .lock()
                    .unwrap()
                    .load(&path)
                    .map(|profiles| profiles.to_vec())
            },
            move |result| {
                if let Some(client) = weak.upgrade() {
                    match result {
                        Ok(profiles) => {
                            *client.profiles.borrow_mut() = profiles;
                            client.writable.set(true);
                            #[cfg(feature = "native-probe")]
                            crate::timing::mark("config-ready");
                            client.render_hosts();
                            client.notice("就绪");
                        }
                        Err(error) => client.error(&error),
                    }
                }
            },
        );
        client
    }

    fn button(button: &gtk::Button, client: &Rc<Self>, action: fn(&Rc<Self>)) {
        let weak = Rc::downgrade(client);
        button.connect_clicked(move |_| {
            if let Some(client) = weak.upgrade() {
                action(&client);
            }
        });
    }
    pub fn notice(&self, message: &str) {
        self.status.style_context().remove_class("error");
        self.status.set_text(message);
    }
    pub fn error(&self, message: &str) {
        self.status.style_context().add_class("error");
        self.status.set_text(message);
    }
    fn confirm(&self, title: &str, message: &str) -> bool {
        let dialog = gtk::MessageDialog::new(
            Some(&self.window),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            gtk::MessageType::Question,
            gtk::ButtonsType::None,
            title,
        );
        dialog.set_widget_name("hshell-confirmation");
        dialog.set_secondary_text(Some(message));
        dialog.add_buttons(&[
            ("取消", gtk::ResponseType::Cancel),
            ("确定", gtk::ResponseType::Accept),
        ]);
        let alive = dialog_guard(&dialog);
        let result = dialog.run() == gtk::ResponseType::Accept;
        dismiss(&dialog, &alive);
        result
    }
    fn quick_profile(&self) -> Result<Profile, String> {
        parse_quick(&self.quick_host.text(), &self.quick_port.text())
    }
    fn quick_connect(self: &Rc<Self>) {
        match self.quick_profile() {
            Ok(profile) => self.connect_profile(profile),
            Err(error) => self.error(&error),
        }
    }
    pub fn active(&self) -> Option<Rc<Tab>> {
        let page = self.notebook.nth_page(self.notebook.current_page())?;
        self.tabs
            .borrow()
            .iter()
            .find(|tab| !tab.closed.get() && tab.root.upcast_ref::<gtk::Widget>() == &page)
            .cloned()
    }
    fn update_session(&self) {
        if self.closing.get() {
            return;
        }
        if let Some(tab) = self.active() {
            self.session_address.set_text(&format!(
                "{} · {}",
                tab.profile.name,
                address(&tab.profile)
            ));
            self.disconnect
                .set_sensitive(tab.phase.get() != Phase::Exited);
            self.reconnect
                .set_sensitive(tab.phase.get() == Phase::Exited);
            tab.terminal.widget.grab_focus();
        } else {
            self.session_address.set_text("选择主机，或输入地址连接");
            self.disconnect.set_sensitive(false);
            self.reconnect.set_sensitive(false);
        }
    }
    fn filter_hosts(self: &Rc<Self>) {
        let query = self.host_search.text().to_lowercase();
        let favorites = self.favorites.is_active();
        for row in self.host_rows.borrow_mut().values_mut() {
            let visible = (!favorites || row.profile.favorite) && row.search.contains(&query);
            if visible != row.visible {
                row.widget.set_visible(visible);
                row.visible = visible;
            }
        }
        self.update_group_visibility();
    }
    fn update_group_visibility(&self) {
        let rows = self.host_rows.borrow();
        for group in self.host_groups.borrow().iter() {
            group.header.set_visible(
                group
                    .rows
                    .iter()
                    .any(|id| rows.get(id).is_some_and(|row| row.visible)),
            );
        }
    }
    #[cfg(feature = "native-probe")]
    pub(crate) fn hosts_ready(&self) -> bool {
        self.writable.get()
            && self.pending_hosts.borrow().is_empty()
            && self.host_source.borrow().is_none()
    }
    #[cfg(feature = "native-probe")]
    pub(crate) async fn benchmark_host_updates(
        self: &Rc<Self>,
    ) -> Result<serde_json::Value, String> {
        use crate::probe::{pause, until};
        use std::{collections::HashMap, time::Instant};
        let mut partial_rows = None;
        if std::env::var_os("HSHELL_NATIVE_HOST_INTERLEAVE").is_some() {
            let count = self.host_rows.borrow().len();
            if self.pending_hosts.borrow().is_empty() || count >= 1000 {
                return Err("Fixture missed progressive host loading".into());
            }
            partial_rows = Some(count);
            let original = self.profiles.borrow().clone();
            let mut changed = original.clone();
            changed[0].favorite = !changed[0].favorite;
            self.host_search.set_text("bench-node-9");
            self.filter_hosts();
            for next in [changed, original] {
                let done = Rc::new(Cell::new(None));
                let completed = done.clone();
                self.persist(next, None, move |ok| completed.set(Some(ok)));
                until(
                    || done.get().is_some() && self.hosts_ready(),
                    "Interleaved host update did not finish",
                )
                .await?;
                if done.get() != Some(true) {
                    return Err("Interleaved host save failed".into());
                }
                let visible = self
                    .host_rows
                    .borrow()
                    .values()
                    .filter(|row| row.visible)
                    .count();
                if self.host_rows.borrow().len() != 1000 || visible != 111 {
                    return Err("Batch cancellation lost/duplicated hosts or ignored search".into());
                }
            }
        }
        self.favorites.set_active(false);
        self.host_search.set_text("");
        self.filter_hosts();
        let selected = self
            .hosts
            .children()
            .into_iter()
            .find(|row| row.widget_name() == "host-bench-500")
            .and_then(|row| row.downcast::<gtk::ListBoxRow>().ok())
            .ok_or("Missing selection fixture")?;
        self.hosts.select_row(Some(&selected));
        let original = self.profiles.borrow().clone();
        let mut changed = original.clone();
        changed[0].favorite = !changed[0].favorite;
        let mut renamed = changed.clone();
        renamed[0].name = "修改后的主机".into();
        renamed[0].group = "Group Z".into();
        renamed[0].host = "renamed-host".into();
        let mut replaced = renamed.clone();
        replaced[999].id = "bench-replacement".into();
        replaced[999].name = "新增主机".into();
        let mut samples = Vec::new();
        for (case, next) in [
            ("favorite", changed),
            ("name-and-group", renamed),
            ("replace-one", replaced),
            ("restore", original),
        ] {
            let before: HashMap<_, _> = self
                .hosts
                .children()
                .into_iter()
                .filter(|row| row.widget_name().starts_with("host-"))
                .map(|row| (row.widget_name().to_string(), row))
                .collect();
            let ticks = Rc::new(RefCell::new(Vec::<f64>::new()));
            let tick_samples = ticks.clone();
            let previous = Rc::new(Cell::new(Instant::now()));
            let timer = glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                tick_samples
                    .borrow_mut()
                    .push(previous.replace(Instant::now()).elapsed().as_secs_f64() * 1000.0);
                glib::ControlFlow::Continue
            });
            let done = Rc::new(Cell::new(None));
            let completed = done.clone();
            let started = Instant::now();
            self.persist(next, None, move |ok| completed.set(Some(ok)));
            let updated = until(
                || done.get().is_some() && self.hosts_ready(),
                "Host update did not finish",
            )
            .await;
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            pause(35).await;
            timer.remove();
            updated?;
            if done.get() != Some(true) {
                return Err("Host update failed".into());
            }
            if self.hosts.selected_row().as_ref() != Some(&selected) {
                return Err("Updating another host lost list selection".into());
            }
            let mut retained = 0;
            let mut replaced = 0;
            for row in self
                .hosts
                .children()
                .into_iter()
                .filter(|row| row.widget_name().starts_with("host-"))
            {
                if let Some(old) = before.get(row.widget_name().as_str()) {
                    if old == &row {
                        retained += 1;
                    } else {
                        replaced += 1;
                    }
                }
            }
            let actual: Vec<_> = self
                .hosts
                .children()
                .into_iter()
                .map(|row| row.widget_name().to_string())
                .collect();
            let mut expected: Vec<_> = self
                .host_order
                .borrow()
                .iter()
                .map(|(key, rank)| (*rank, key.clone()))
                .collect();
            expected.sort();
            if actual != expected.into_iter().map(|(_, key)| key).collect::<Vec<_>>() {
                return Err("Incremental host/group order is incorrect".into());
            }
            for row in self.host_rows.borrow().values() {
                if row.name.text() != row.profile.name
                    || row.address.text() != address(&row.profile)
                {
                    return Err("Host labels were not updated".into());
                }
            }
            samples.push(serde_json::json!({"case":case,"elapsedMs":elapsed,"retainedRows":retained,"replacedRows":replaced,"maxLoopTickMs":ticks.borrow().iter().copied().fold(0.0_f64, f64::max)}));
            if case == "name-and-group" {
                let edit = {
                    let rows = self.host_rows.borrow();
                    let content = rows["bench-0"]
                        .widget
                        .child()
                        .unwrap()
                        .downcast::<gtk::Box>()
                        .unwrap();
                    content
                        .children()
                        .into_iter()
                        .filter_map(|widget| widget.downcast::<gtk::Button>().ok())
                        .find(|button| button.tooltip_text().as_deref() == Some("编辑连接"))
                        .unwrap()
                };
                edit.emit_clicked();
                let editor = gtk::Window::list_toplevels()
                    .into_iter()
                    .find(|window| window.widget_name() == "hshell-profile-editor")
                    .and_then(|window| window.downcast::<gtk::Dialog>().ok())
                    .ok_or("Edit callback did not open editor")?;
                let name = crate::probe::field::<gtk::Entry>(&editor, "profile-name")?.text();
                let host = crate::probe::field::<gtk::Entry>(&editor, "profile-host")?.text();
                editor.response(gtk::ResponseType::Cancel);
                if name != "修改后的主机" || host != "renamed-host" {
                    return Err("Retained row edited stale profile data".into());
                }
                pause(150).await;
            }
        }
        Ok(
            serde_json::json!({"phase":"complete","profiles":self.profiles.borrow().len(),"updates":samples,"initialPartialRows":partial_rows}),
        )
    }
    #[cfg(feature = "native-probe")]
    pub(crate) fn benchmark_hosts(self: &Rc<Self>) -> Result<serde_json::Value, String> {
        let mut samples = Vec::new();
        for _ in 0..3 {
            for (query, expected) in [
                ("bench-node-9", 111),
                ("missing-host", 0),
                ("", 1000),
                ("bench-node-500", 1),
                ("BENCH-NODE-9", 111),
            ] {
                self.host_search.set_text(query);
                let started = std::time::Instant::now();
                self.filter_hosts();
                let milliseconds = started.elapsed().as_secs_f64() * 1000.0;
                let count = self
                    .hosts
                    .children()
                    .iter()
                    .filter(|row| row.is_visible() && row.widget_name().starts_with("host-"))
                    .count();
                if count != expected {
                    return Err(format!(
                        "Host filtering {query:?}: {count}, expected {expected}"
                    ));
                }
                samples.push(
                    serde_json::json!({"query":query,"visible":count,"milliseconds":milliseconds}),
                );
            }
        }
        self.host_search.set_text("");
        self.favorites.set_active(true);
        self.filter_hosts();
        let favorites = self
            .hosts
            .children()
            .iter()
            .filter(|row| row.is_visible() && row.widget_name().starts_with("host-"))
            .count();
        if favorites != 500 {
            return Err(format!("Favorites filter: {favorites}, expected 500"));
        }
        Ok(
            serde_json::json!({"phase":"complete","profiles":1000,"favorites":favorites,"samples":samples}),
        )
    }
    fn render_hosts(self: &Rc<Self>) {
        if let Some(source) = self.host_source.borrow_mut().take() {
            source.remove();
        }
        let mut profiles = self.profiles.borrow().clone();
        profiles.sort_by(|a, b| {
            a.group
                .cmp(&b.group)
                .then(a.name.cmp(&b.name))
                .then(a.id.cmp(&b.id))
        });
        let ids: HashSet<_> = profiles.iter().map(|profile| profile.id.as_str()).collect();
        self.host_rows.borrow_mut().retain(|id, row| {
            if ids.contains(id.as_str()) {
                true
            } else {
                self.hosts.remove(&row.widget);
                false
            }
        });
        let mut old: HashMap<_, _> = self
            .host_groups
            .borrow_mut()
            .drain(..)
            .map(|group| (group.name.clone(), group))
            .collect();
        let mut groups: Vec<HostGroup> = Vec::new();
        let mut order = HashMap::new();
        let mut position = 0;
        let mut pending = VecDeque::new();
        let mut headers = Vec::new();
        let mut reordered = Vec::new();
        {
            let mut rows = self.host_rows.borrow_mut();
            for profile in &profiles {
                if groups
                    .last()
                    .is_none_or(|group| group.name != profile.group)
                {
                    let mut group = old.remove(&profile.group).unwrap_or_else(|| {
                        let header = gtk::ListBoxRow::new();
                        header.set_widget_name(&format!("group-{}", profile.group));
                        header.set_selectable(false);
                        header.set_activatable(false);
                        header.add(&label(
                            if profile.group.is_empty() {
                                "未分组"
                            } else {
                                &profile.group
                            },
                            "group",
                        ));
                        headers.push(header.clone());
                        HostGroup {
                            name: profile.group.clone(),
                            header,
                            rows: Vec::new(),
                        }
                    });
                    group.rows.clear();
                    order.insert(group.header.widget_name().to_string(), position);
                    position += 1;
                    groups.push(group);
                }
                groups.last_mut().unwrap().rows.push(profile.id.clone());
                order.insert(format!("host-{}", profile.id), position);
                position += 1;
                if let Some(row) = rows.get_mut(&profile.id) {
                    if row.profile.group != profile.group || row.profile.name != profile.name {
                        reordered.push(row.widget.clone());
                    }
                    if row.profile.name != profile.name {
                        row.name.set_text(&profile.name);
                    }
                    if address(&row.profile) != address(profile) {
                        row.address.set_text(&address(profile));
                    }
                    if row.profile.favorite != profile.favorite {
                        row.favorite_button
                            .set_image(Some(&icon(if profile.favorite {
                                "starred-symbolic"
                            } else {
                                "non-starred-symbolic"
                            })));
                    }
                    row.search = Self::host_search_text(profile);
                    row.profile = profile.clone();
                } else {
                    pending.push_back(profile.clone());
                }
            }
        }
        for group in old.into_values() {
            self.hosts.remove(&group.header);
        }
        *self.host_order.borrow_mut() = order;
        *self.host_groups.borrow_mut() = groups;
        *self.pending_hosts.borrow_mut() = pending;
        // Numeric ranks of following rows can shift without changing their order.
        // Only rows whose actual sort key changed need GTK reinsertion.
        for row in reordered {
            row.changed();
        }
        for header in headers {
            header.show_all();
            self.hosts.add(&header);
        }
        self.filter_hosts();
        // Existing rows update synchronously. New rows are bounded per main-loop turn.
        if self.render_host_batch() {
            let weak = Rc::downgrade(self);
            let source = glib::idle_add_local(move || {
                let Some(client) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if client.closing.get() || !client.render_host_batch() {
                    client.host_source.borrow_mut().take();
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
            *self.host_source.borrow_mut() = Some(source);
        }
    }
    fn host_search_text(profile: &Profile) -> String {
        format!(
            "{} {} {} {}",
            profile.name, profile.host, profile.username, profile.group
        )
        .to_lowercase()
    }
    fn render_host_batch(self: &Rc<Self>) -> bool {
        let started = std::time::Instant::now();
        let query = self.host_search.text().to_lowercase();
        let favorites = self.favorites.is_active();
        for _ in 0..16 {
            let Some(profile) = self.pending_hosts.borrow_mut().pop_front() else {
                break;
            };
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("host-{}", profile.id));
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.add(&content);
            content.pack_start(&icon("network-server-symbolic"), false, false, 0);
            let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
            let name = label(&profile.name, "");
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let host = label(&address(&profile), "address");
            host.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            text.pack_start(&name, false, false, 0);
            text.pack_start(&host, false, false, 0);
            content.pack_start(&text, true, true, 0);
            let favorite = icon_button(
                if profile.favorite {
                    "starred-symbolic"
                } else {
                    "non-starred-symbolic"
                },
                "切换收藏",
            );
            content.pack_end(&favorite, false, false, 0);
            let edit = icon_button("document-edit-symbolic", "编辑连接");
            content.pack_end(&edit, false, false, 0);
            let weak = Rc::downgrade(self);
            let id = profile.id.clone();
            edit.connect_clicked(move |_| {
                if let Some(client) = weak.upgrade() {
                    let profile = client
                        .profiles
                        .borrow()
                        .iter()
                        .find(|profile| profile.id == id)
                        .cloned();
                    if let Some(profile) = profile {
                        client.editor(Some(profile));
                    }
                }
            });
            let weak = Rc::downgrade(self);
            let id = profile.id.clone();
            favorite.connect_clicked(move |_| {
                if let Some(client) = weak.upgrade() {
                    let mut next = client.profiles.borrow().clone();
                    if let Some(profile) = next.iter_mut().find(|p| p.id == id) {
                        profile.favorite = !profile.favorite;
                    }
                    client.persist(next, None, |_| {});
                }
            });
            let search = Self::host_search_text(&profile);
            let visible = (!favorites || profile.favorite) && search.contains(&query);
            row.show_all();
            row.set_visible(visible);
            self.hosts.add(&row);
            self.host_rows.borrow_mut().insert(
                profile.id.clone(),
                HostRow {
                    widget: row,
                    profile,
                    name,
                    address: host,
                    favorite_button: favorite,
                    search,
                    visible,
                },
            );
            if started.elapsed() >= std::time::Duration::from_millis(6) {
                break;
            }
        }
        self.update_group_visibility();
        let pending = !self.pending_hosts.borrow().is_empty();
        #[cfg(feature = "native-probe")]
        if !pending {
            crate::timing::mark("host-list-ready");
        }
        pending
    }

    #[allow(deprecated)]
    pub fn connect_profile(self: &Rc<Self>, profile: Profile) {
        if self.closing.get() {
            return;
        }
        if let Some(source) = self.reclaim_source.borrow_mut().take() {
            source.remove();
        }
        if self.tabs.borrow().len() >= 12 {
            self.error("最多保留 12 个终端标签，请先关闭不再使用的会话。");
            return;
        }
        if let Err(error) = profile.validate() {
            self.error(&error);
            return;
        }
        let terminal = Terminal::new();
        terminal.widget.set_margin_start(14);
        terminal.widget.set_margin_end(8);
        terminal.widget.set_margin_top(10);
        terminal.widget.set_margin_bottom(8);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.add(&terminal.widget);
        root.pack_start(&scroll, true, true, 0);
        let banner = label("", "dim");
        banner.set_margin_start(16);
        banner.set_margin_top(8);
        banner.set_margin_bottom(8);
        banner.set_no_show_all(true);
        root.pack_end(&banner, false, false, 0);
        let tab_label = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let title = label(&profile.name, "");
        title.set_width_chars(14);
        title.set_max_width_chars(22);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        tab_label.pack_start(&title, true, true, 0);
        let close = icon_button("window-close-symbolic", "关闭会话");
        tab_label.pack_end(&close, false, false, 0);
        tab_label.show_all();
        let tab = Rc::new(Tab {
            id: uuid::Uuid::new_v4().to_string(),
            profile: profile.clone(),
            terminal,
            phase: Cell::new(Phase::Starting),
            closed: Cell::new(false),
            session: RefCell::new(None),
            root,
            label: title,
            banner,
            pending_auth: RefCell::new(VecDeque::new()),
        });
        let page = self.notebook.append_page(&tab.root, Some(&tab_label));
        self.notebook.set_tab_reorderable(&tab.root, true);
        tab.root.show_all();
        tab.banner.hide();
        self.tabs.borrow_mut().push(tab.clone());
        self.notebook.set_current_page(Some(page));
        let weak = Rc::downgrade(self);
        let id = tab.id.clone();
        close.connect_clicked(move |_| {
            if let Some(client) = weak.upgrade() {
                client.close_tab(&id, true);
            }
        });
        self.bind_terminal(&tab);
        let (sender, receiver) = glib::MainContext::channel(glib::Priority::DEFAULT);
        let weak = Rc::downgrade(self);
        let weak_tab = Rc::downgrade(&tab);
        receiver.attach(None, move |event| {
            let (Some(client), Some(tab)) = (weak.upgrade(), weak_tab.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if tab.closed.get() {
                return glib::ControlFlow::Break;
            }
            client.session_event(&tab, event);
            glib::ControlFlow::Continue
        });
        let path = self.profiles_path.clone();
        let lock = self.config_lock.clone();
        let cache = self.profile_cache.clone();
        let weak = Rc::downgrade(self);
        let weak_tab = Rc::downgrade(&tab);
        job(
            move || {
                let saved = {
                    let _guard = lock.lock().unwrap();
                    cache
                        .lock()
                        .unwrap()
                        .load(&path)?
                        .iter()
                        .find(|p| {
                            p.id == profile.id
                                && p.host == profile.host
                                && p.username == profile.username
                                && p.port == profile.port
                        })
                        .cloned()
                };
                let password = saved
                    .filter(|p| p.password_saved)
                    .map(|p| ssh_core::password_for(&ssh_core::SystemCredentials, &p))
                    .transpose()?;
                DirectSession::start(&profile, password, 100, 30, move |event| {
                    sender.send(event).is_ok()
                })
            },
            move |result| {
                let (Some(client), Some(tab)) = (weak.upgrade(), weak_tab.upgrade()) else {
                    return;
                };
                if tab.closed.get() || client.closing.get() {
                    return;
                }
                match result {
                    Ok(session) => {
                        match session.terminal_fd().and_then(|fd| tab.terminal.attach(fd)) {
                            Ok(()) => {
                                *tab.session.borrow_mut() = Some(session);
                                if tab.phase.get() == Phase::Starting {
                                    tab.phase.set(Phase::Running);
                                }
                                let pending: Vec<_> =
                                    tab.pending_auth.borrow_mut().drain(..).collect();
                                for (request_id, prompt, confirm) in pending {
                                    client.authenticate(&tab, request_id, prompt, confirm);
                                }
                                client.update_session();
                                client.notice("会话运行中");
                            }
                            Err(error) => {
                                tab.phase.set(Phase::Exited);
                                client.error(&error);
                            }
                        }
                    }
                    Err(error) => {
                        tab.phase.set(Phase::Exited);
                        tab.terminal.message(&format!("\x1b[31m{error}\x1b[0m\r\n"));
                        tab.banner.set_text(&error);
                        tab.banner.show();
                        client.error(&error);
                        client.update_session();
                    }
                }
            },
        );
        self.update_session();
    }

    fn session_event(self: &Rc<Self>, tab: &Rc<Tab>, event: SessionEvent) {
        match event {
            SessionEvent::Auth {
                request_id,
                prompt,
                confirm,
            } => {
                if tab.session.borrow().is_none() {
                    tab.pending_auth
                        .borrow_mut()
                        .push_back((request_id, prompt, confirm));
                } else {
                    self.authenticate(tab, request_id, prompt, confirm);
                }
            }
            SessionEvent::Exit { code, error } => {
                tab.phase.set(Phase::Exited);
                tab.terminal.disable_input();
                tab.session.borrow_mut().take();
                tab.label
                    .set_text(&format!("{} · 已结束", tab.profile.name));
                tab.banner.set_text(&error.unwrap_or_else(|| {
                    format!(
                        "SSH 进程已结束（退出码 {}）。输出已保留，可关闭标签或重新连接。",
                        code.map_or_else(|| "未知".into(), |c| c.to_string())
                    )
                }));
                tab.banner.show();
                self.update_session();
            }
            SessionEvent::Data(_) => unreachable!("VTE consumes PTY output directly"),
        }
    }
    fn authenticate(
        self: &Rc<Self>,
        tab: &Rc<Tab>,
        request_id: String,
        prompt: String,
        confirm: bool,
    ) {
        if tab.closed.get() || tab.phase.get() == Phase::Exited {
            return;
        }
        let dialog = gtk::Dialog::with_buttons(
            Some(if confirm {
                "确认主机指纹"
            } else {
                "SSH 身份认证"
            }),
            Some(&self.window),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            &[
                ("取消", gtk::ResponseType::Cancel),
                ("确定", gtk::ResponseType::Accept),
            ],
        );
        dialog.set_widget_name("hshell-authentication");
        let content = dialog.content_area();
        content.style_context().add_class("dialog-content");
        content.set_spacing(12);
        let text = label(&prompt, "");
        text.set_selectable(true);
        text.set_line_wrap(true);
        text.set_max_width_chars(70);
        content.add(&text);
        let password = entry("输入认证口令", 4096);
        password.set_widget_name("authentication-password");
        password.set_visibility(false);
        password.set_activates_default(true);
        if !confirm {
            content.add(&password);
        }
        dialog.set_default_response(gtk::ResponseType::Accept);
        dialog.show_all();
        if !confirm {
            password.grab_focus();
        }
        let alive = dialog_guard(&dialog);
        let accepted = dialog.run() == gtk::ResponseType::Accept;
        if !alive.get() {
            return;
        }
        let answer = accepted.then(|| {
            if confirm {
                "yes".into()
            } else {
                password.text().to_string()
            }
        });
        password.set_text("");
        dismiss(&dialog, &alive);
        if let Some(session) = tab.session.borrow().as_ref() {
            if let Err(error) = session.respond(&request_id, answer) {
                self.error(&error);
            }
            if !accepted {
                session.close();
            }
        }
    }
    pub fn close_tab(self: &Rc<Self>, id: &str, ask: bool) {
        let tab = self.tabs.borrow().iter().find(|tab| tab.id == id).cloned();
        let Some(tab) = tab else {
            return;
        };
        if ask
            && tab.phase.get() != Phase::Exited
            && !self.confirm(
                "关闭这个会话？",
                "SSH 连接将断开。需要继续的远端任务请先放入 tmux 或 screen。",
            )
        {
            return;
        }
        tab.closed.set(true);
        tab.terminal.dispose();
        tab.session.borrow_mut().take();
        if let Some(page) = self.notebook.page_num(&tab.root) {
            self.notebook.remove_page(Some(page));
        }
        self.tabs.borrow_mut().retain(|item| item.id != id);
        // Coalesce a burst of closed tabs, including when other sessions remain.
        // Opening another tab cancels this task before it starts.
        if let Some(source) = self.reclaim_source.borrow_mut().take() {
            source.remove();
        }
        let weak = Rc::downgrade(self);
        let source =
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                if let Some(client) = weak.upgrade() {
                    client.reclaim_source.borrow_mut().take();
                    if !client.closing.get() {
                        crate::reclaim::idle_pages();
                    }
                }
            });
        *self.reclaim_source.borrow_mut() = Some(source);
        self.update_session();
        self.notice("会话已关闭");
    }
    pub(crate) fn open_files(&self, profile: Profile) -> Rc<crate::files::FilePane> {
        if self.files.borrow().is_none() {
            let pane = crate::files::FilePane::new(&self.window, self.profiles_path.clone());
            self.terminal_pane.pack2(&pane.root, false, false);
            *self.files.borrow_mut() = Some(pane);
        }
        let pane = self.files.borrow().as_ref().unwrap().clone();
        // Keep the terminal usable at the supported minimum window size.
        if self.window.allocated_width() < 1050 {
            self.hide_sidebar();
            self.terminal_pane
                .set_position((self.window.allocated_width() - 360).max(300));
        }
        pane.show(profile);
        pane
    }
    fn toggle_sidebar(&self) {
        if self.sidebar.is_visible() {
            self.hide_sidebar();
        } else {
            if self.window.allocated_width() < 1050 {
                if let Some(files) = self.files.borrow().as_ref() {
                    files.hide();
                }
            }
            self.sidebar.set_no_show_all(false);
            self.sidebar.show();
            self.host_pane.set_position(244);
        }
    }
    fn hide_sidebar(&self) {
        self.sidebar.set_no_show_all(true);
        self.sidebar.hide();
        self.host_pane.set_position(0);
    }
    #[cfg(feature = "native-probe")]
    pub(crate) fn file_layout(&self) -> serde_json::Value {
        serde_json::json!({"sidebarVisible":self.sidebar.is_visible(),"sidebarWidth":self.sidebar.allocated_width(),"hostPosition":self.host_pane.position(),"hostWidth":self.host_pane.allocated_width(),"terminalPosition":self.terminal_pane.position(),"terminalPaneWidth":self.terminal_pane.allocated_width(),"window":self.window.allocated_width()})
    }
    pub fn shutdown(&self) {
        self.closing.set(true);
        if let Some(files) = self.files.borrow().as_ref() {
            files.stop();
        }
        if let Some(source) = self.host_source.borrow_mut().take() {
            source.remove();
        }
        self.pending_hosts.borrow_mut().clear();
        if let Some(source) = self.reclaim_source.borrow_mut().take() {
            source.remove();
        }
        for tab in self.tabs.borrow_mut().drain(..) {
            tab.closed.set(true);
            tab.terminal.dispose();
            tab.session.borrow_mut().take();
        }
    }

    fn copy(&self) {
        if let Some(tab) = self.active() {
            tab.terminal.copy();
        }
    }
    pub(crate) fn paste(self: &Rc<Self>, tab: &Rc<Tab>, primary: bool) {
        let weak = Rc::downgrade(self);
        let weak_tab = Rc::downgrade(tab);
        gtk::Clipboard::get(if primary {
            &gdk::SELECTION_PRIMARY
        } else {
            &gdk::SELECTION_CLIPBOARD
        })
        .request_text(move |_, text| {
            let (Some(client), Some(tab), Some(text)) = (weak.upgrade(), weak_tab.upgrade(), text)
            else {
                return;
            };
            if tab.closed.get() || tab.phase.get() != Phase::Running {
                return;
            }
            if text.len() > 1024 * 1024 {
                client.error("粘贴内容超过 1 MiB，请通过文件传输处理。");
                return;
            }
            if (text.contains('\n') || text.contains('\r'))
                && !client.confirm(
                    "确认粘贴多行内容？",
                    "换行可能立即执行远端命令。请确认剪贴板内容可信。",
                )
            {
                return;
            }
            if !tab.closed.get() {
                tab.terminal.paste(text);
            }
        });
    }
    fn bind_terminal(self: &Rc<Self>, tab: &Rc<Tab>) {
        let weak = Rc::downgrade(self);
        let weak_tab = Rc::downgrade(tab);
        tab.terminal
            .widget
            .connect_button_press_event(move |_, event| {
                let (Some(client), Some(tab)) = (weak.upgrade(), weak_tab.upgrade()) else {
                    return glib::Propagation::Proceed;
                };
                if event.button() == 2 {
                    client.paste(&tab, true);
                    return glib::Propagation::Stop;
                }
                if event.button() == 3 {
                    let menu = gtk::Menu::new();
                    for (title, action) in [("复制", 0), ("粘贴", 1), ("查找", 2)] {
                        let item = gtk::MenuItem::with_label(title);
                        let weak = Rc::downgrade(&client);
                        let weak_tab = Rc::downgrade(&tab);
                        item.connect_activate(move |_| {
                            if let (Some(client), Some(tab)) = (weak.upgrade(), weak_tab.upgrade())
                            {
                                match action {
                                    0 => client.copy(),
                                    1 => client.paste(&tab, false),
                                    _ => client.show_search(),
                                }
                            }
                        });
                        menu.append(&item);
                    }
                    menu.show_all();
                    menu.popup_at_pointer(Some(event));
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
    }
    fn show_search(&self) {
        if self.active().is_some() {
            self.search_row.set_no_show_all(false);
            self.search_row.show_all();
            self.search_row.set_no_show_all(true);
            self.search_input.grab_focus();
        }
    }
    fn find(&self, previous: bool) {
        if let Some(tab) = self.active() {
            let text = self.search_input.text();
            let found = tab.terminal.find(&text, previous);
            self.search_result.set_text(if found || text.is_empty() {
                ""
            } else {
                "未找到"
            });
        }
    }
    fn bind_keys(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.window.connect_key_press_event(move |_, event| {
            let Some(client) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let modifiers = event.state();
            let ctrl = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = modifiers.contains(gdk::ModifierType::SHIFT_MASK);
            let alt = modifiers.contains(gdk::ModifierType::MOD1_MASK);
            let key = event
                .keyval()
                .to_unicode()
                .map(|key| key.to_ascii_lowercase());
            if ctrl && shift {
                match key {
                    Some('k') => client.quick_host.grab_focus(),
                    Some('b') => client.toggle_sidebar(),
                    Some('c') => client.copy(),
                    Some('v') => {
                        if let Some(tab) = client.active() {
                            client.paste(&tab, false);
                        }
                    }
                    Some('f') => client.show_search(),
                    Some('t') => client.editor(None),
                    Some('w') => {
                        if let Some(tab) = client.active() {
                            client.close_tab(&tab.id, true);
                        }
                    }
                    _ => return glib::Propagation::Proceed,
                }
                return glib::Propagation::Stop;
            }
            if ctrl && !shift {
                if client
                    .active()
                    .is_some_and(|tab| tab.terminal.widget.has_focus())
                {
                    return glib::Propagation::Proceed;
                }
                match key {
                    Some('k') => client.quick_host.grab_focus(),
                    Some('b') => client.toggle_sidebar(),
                    _ => return glib::Propagation::Proceed,
                }
                return glib::Propagation::Stop;
            }
            if shift && event.keyval() == gdk::keys::constants::Insert {
                if let Some(tab) = client.active() {
                    client.paste(&tab, false);
                }
                return glib::Propagation::Stop;
            }
            if alt {
                if let Some(digit) = key
                    .and_then(|key| key.to_digit(10))
                    .filter(|digit| (1..=9).contains(digit))
                {
                    let pages: Vec<_> = (0..client.notebook.n_pages())
                        .filter(|index| {
                            client.notebook.nth_page(Some(*index)).is_some_and(|page| {
                                client
                                    .tabs
                                    .borrow()
                                    .iter()
                                    .any(|tab| tab.root.upcast_ref::<gtk::Widget>() == &page)
                            })
                        })
                        .collect();
                    if let Some(page) = pages.get(digit as usize - 1) {
                        client.notebook.set_current_page(Some(*page));
                    }
                    return glib::Propagation::Stop;
                }
            }
            if event.keyval() == gdk::keys::constants::Escape && client.search_row.is_visible() {
                client.search_row.hide();
                if let Some(tab) = client.active() {
                    tab.terminal.find("", false);
                    tab.terminal.widget.grab_focus();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
    }
    fn info(&self) {
        let dialog = gtk::MessageDialog::new(
            Some(&self.window),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            gtk::MessageType::Info,
            gtk::ButtonsType::Close,
            &format!("HShell {}", env!("CARGO_PKG_VERSION")),
        );
        dialog.set_secondary_text(Some(&format!("Ubuntu 原生客户端 · GTK / VTE / Rust / OpenSSH\n\nCtrl Shift K  快速连接\nCtrl Shift B  显示 / 隐藏主机栏\nCtrl Shift T  新建连接\nCtrl Shift W  关闭会话\nCtrl Shift F  搜索终端\nCtrl Shift C / V  复制 / 粘贴\nAlt 1 … 9  切换终端\n\n连接配置：{}\n密码保存在本机系统凭据库。", self.profiles_path.display())));
        let alive = dialog_guard(&dialog);
        dialog.run();
        dismiss(&dialog, &alive);
    }

    fn persist(
        self: &Rc<Self>,
        next: Vec<Profile>,
        update: Option<PasswordUpdate>,
        done: impl FnOnce(bool) + 'static,
    ) {
        if !self.writable.get() {
            self.error("连接配置尚未成功读取，已禁止覆盖。");
            done(false);
            return;
        }
        if self.saving.replace(true) {
            self.error("正在保存，请稍后重试。");
            done(false);
            return;
        }
        let path = self.profiles_path.clone();
        let lock = self.config_lock.clone();
        let weak = Rc::downgrade(self);
        job(
            move || {
                let _guard = lock.lock().unwrap();
                ssh_core::store_connections(&path, next, update, &ssh_core::SystemCredentials)
            },
            move |result| {
                if let Some(client) = weak.upgrade() {
                    client.saving.set(false);
                    match result {
                        Ok(profiles) => {
                            *client.profiles.borrow_mut() = profiles;
                            client.render_hosts();
                            client.notice("连接已保存");
                            done(true);
                        }
                        Err(error) => {
                            client.error(&error);
                            done(false);
                        }
                    }
                }
            },
        );
    }
    pub(crate) fn editor(self: &Rc<Self>, initial: Option<Profile>) {
        let existing = initial.as_ref().and_then(|profile| {
            self.profiles
                .borrow()
                .iter()
                .find(|item| item.id == profile.id)
                .cloned()
        });
        let initial = initial.unwrap_or_else(|| Profile {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            host: String::new(),
            username: String::new(),
            port: None,
            identity_file: String::new(),
            group: String::new(),
            favorite: false,
            password_saved: false,
        });
        let dialog = gtk::Dialog::with_buttons(
            Some(if existing.is_some() {
                "编辑连接"
            } else {
                "新建连接"
            }),
            Some(&self.window),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            &[
                ("取消", gtk::ResponseType::Cancel),
                ("保存连接", gtk::ResponseType::Accept),
            ],
        );
        dialog.set_default_size(470, 620);
        dialog.set_widget_name("hshell-profile-editor");
        dialog.set_size_request(400, 400);
        dialog.set_resizable(true);
        if existing.is_some() {
            dialog.add_button("删除连接", gtk::ResponseType::Reject);
        }
        if let Some(save) = dialog.widget_for_response(gtk::ResponseType::Accept) {
            save.style_context().add_class("primary");
        }
        let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_propagate_natural_height(true);
        scroll.set_max_content_height(650);
        scroll.set_vexpand(true);
        let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
        form.style_context().add_class("dialog-content");
        form.pack_start(
            &label("保存常用服务器。密码可加密保存在本机系统凭据库。", "dim"),
            false,
            false,
            6,
        );
        let name = entry("例如：开发服务器", 80);
        name.set_widget_name("profile-name");
        name.set_text(&initial.name);
        let host = entry("192.168.1.10 或 SSH 别名", 253);
        host.set_widget_name("profile-host");
        host.set_text(&initial.host);
        let username = entry("留空沿用 SSH 配置", 128);
        username.set_widget_name("profile-username");
        username.set_text(&initial.username);
        let port = entry("留空沿用配置 / 22", 5);
        port.set_text(
            &initial
                .port
                .map_or_else(String::new, |port| port.to_string()),
        );
        port.set_input_purpose(gtk::InputPurpose::Digits);
        let password = entry(
            if initial.password_saved {
                "留空保持已保存的密码"
            } else {
                "输入要保存的密码"
            },
            4096,
        );
        password.set_widget_name("profile-password");
        password.set_visibility(false);
        password.set_input_purpose(gtk::InputPurpose::Password);
        let key = entry("~/.ssh/id_ed25519（留空自动选择）", 4096);
        key.set_text(&initial.identity_file);
        let group = entry("例如：工作", 40);
        group.set_text(&initial.group);
        for (title, input) in [
            ("连接名称", &name),
            ("主机地址或 SSH 别名", &host),
            ("用户名（可选）", &username),
            ("端口（可选）", &port),
            ("登录密码（可选）", &password),
        ] {
            form.pack_start(&label(title, ""), false, false, 0);
            form.pack_start(input, false, false, 0);
        }
        let remember = gtk::CheckButton::with_label("保存密码到本机系统凭据库");
        remember.set_widget_name("profile-remember");
        remember.set_active(initial.password_saved);
        form.pack_start(&remember, false, false, 0);
        let remember_on_type = remember.clone();
        password.connect_changed(move |input| {
            if !input.text().is_empty() {
                remember_on_type.set_active(true);
            }
        });
        for (title, input) in [("私钥路径（可选）", &key), ("分组（可选）", &group)] {
            form.pack_start(&label(title, ""), false, false, 0);
            form.pack_start(input, false, false, 0);
        }
        let favorite = gtk::CheckButton::with_label("收藏连接");
        favorite.set_active(initial.favorite);
        form.pack_start(&favorite, false, false, 0);
        let error = label("", "error");
        error.set_widget_name("profile-error");
        error.set_line_wrap(true);
        error.set_max_width_chars(48);
        form.pack_start(&error, false, false, 4);
        scroll.add(&form);
        dialog.content_area().pack_start(&scroll, true, true, 0);
        let weak = Rc::downgrade(self);
        let alive = dialog_guard(&dialog);
        let initial_focus = name.clone();
        dialog.connect_response(move |dialog, response| {
            if !alive.get() { return; }
            let Some(client) = weak.upgrade() else { dismiss(dialog, &alive); return; };
            if response == gtk::ResponseType::Accept {
                let parsed: Result<(Profile, Option<PasswordUpdate>), String> = (|| {
                    let port = parse_port(&port.text())?;
                    let profile = Profile { id: initial.id.clone(), name: name.text().trim().into(), host: host.text().trim().trim_start_matches('[').trim_end_matches(']').into(), username: username.text().trim().into(), port, identity_file: key.text().trim().into(), group: group.text().trim().into(), favorite: favorite.is_active(), password_saved: false };
                    profile.validate()?;
                    let update = if remember.is_active() {
                        if password.text().is_empty() {
                            let same = existing.as_ref().is_some_and(|old| old.password_saved && old.host == profile.host && old.username == profile.username && old.port == profile.port);
                            if !same { return Err("请填写要保存的密码；更改目标后需要重新输入。".into()); } None
                        } else { Some(PasswordUpdate::Set { id: profile.id.clone(), password: password.text().to_string() }) }
                    } else if existing.as_ref().is_some_and(|old| old.password_saved) { Some(PasswordUpdate::Remove { id: profile.id.clone() }) } else { None };
                    Ok((profile, update))
                })();
                match parsed {
                    Ok((profile, update)) => {
                        let mut next = client.profiles.borrow().clone(); if let Some(item) = next.iter_mut().find(|item| item.id == profile.id) { *item = profile; } else { next.push(profile); }
                        let dialog = dialog.clone(); let password = password.clone(); let error = error.clone(); let alive = alive.clone();
                        dialog.set_response_sensitive(gtk::ResponseType::Accept, false);
                        client.persist(next, update, move |ok| { if !alive.get() { return; } password.set_text(""); if ok { dismiss(&dialog, &alive); } else { dialog.set_response_sensitive(gtk::ResponseType::Accept, true); error.set_text("保存失败，请查看主窗口底部的错误提示；可以取消密码保存后重试。"); } });
                    },
                    Err(message) => error.set_text(&message),
                }
            } else if response == gtk::ResponseType::Reject {
                if client.confirm("删除这个连接？", "连接配置和已保存的凭据将一并删除，已打开的终端会保留。") {
                    let next = client.profiles.borrow().iter().filter(|profile| profile.id != initial.id).cloned().collect(); let dialog = dialog.clone(); let alive = alive.clone();
                    client.persist(next, None, move |ok| { if ok && alive.get() { dismiss(&dialog, &alive); } });
                }
            } else { password.set_text(""); dismiss(dialog, &alive); }
        });
        dialog.show_all();
        initial_focus.grab_focus();
    }
}

fn address(profile: &Profile) -> String {
    format!(
        "{}{}{}{}",
        profile.username,
        if profile.username.is_empty() { "" } else { "@" },
        profile.host,
        profile
            .port
            .map_or_else(String::new, |port| format!(":{port}"))
    )
}
fn parse_port(port: &str) -> Result<Option<u16>, String> {
    if port.trim().is_empty() {
        return Ok(None);
    }
    port.trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .map(Some)
        .ok_or_else(|| "端口范围为 1–65535".into())
}
fn parse_quick(text: &str, port: &str) -> Result<Profile, String> {
    let text = text.trim();
    let (username, host) = text.rsplit_once('@').unwrap_or(("", text));
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let profile = Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name: host.to_string(),
        host: host.to_string(),
        username: username.to_string(),
        port: parse_port(port)?,
        identity_file: String::new(),
        group: String::new(),
        favorite: false,
        password_saved: false,
    };
    profile.validate()?;
    Ok(profile)
}
fn load_fonts() {
    use std::ffi::CString;
    #[link(name = "fontconfig")]
    extern "C" {
        fn FcConfigAppFontAddFile(config: *mut std::ffi::c_void, path: *const u8) -> i32;
    }
    let installed = std::env::current_exe().ok().and_then(|exe| {
        exe.parent()
            .map(|parent| parent.join("../lib/hshell/fonts"))
    });
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../native-assets/fonts");
    let directory = installed.filter(|path| path.is_dir()).unwrap_or(source);
    for file in [
        "ibm-plex-sans.ttf",
        "ibm-plex-sans-medium.ttf",
        "jetbrains-mono.ttf",
        "jetbrains-mono-semibold.ttf",
    ] {
        if let Ok(path) = CString::new(directory.join(file).as_os_str().as_encoded_bytes()) {
            unsafe {
                FcConfigAppFontAddFile(std::ptr::null_mut(), path.as_ptr().cast());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quick_alias_preserves_config() {
        let profile = parse_quick(" my-alias ", "").unwrap();
        assert_eq!(profile.host, "my-alias");
        assert_eq!(profile.username, "");
        assert_eq!(profile.port, None);
    }
    #[test]
    fn quick_ipv6_and_user() {
        let profile = parse_quick("dev@[2001:db8::1]", "2222").unwrap();
        assert_eq!(profile.host, "2001:db8::1");
        assert_eq!(profile.username, "dev");
        assert_eq!(profile.port, Some(2222));
    }
    #[test]
    fn rejects_invalid_connection() {
        for host in ["", "-oProxyCommand=bad", "user@host;bad", "user@host/name"] {
            assert!(parse_quick(host, "").is_err());
        }
        for port in ["0", "65536", "22a"] {
            assert!(parse_quick("host", port).is_err());
        }
    }
}
