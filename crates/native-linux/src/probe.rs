//! Development-only fixture. Never compiled into the installation package.
use crate::ui::{Client, Phase};
use gtk::{gdk, glib, prelude::*};
use serde_json::json;
use ssh_core::{CredentialStore, Profile, SystemCredentials};
use std::{
    cell::Cell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

pub(crate) async fn pause(ms: u64) {
    glib::timeout_future(Duration::from_millis(ms)).await;
}
pub(crate) async fn startup(client: &Client, result: &std::path::Path) -> Result<(), String> {
    until(
        || client.hosts_ready() && crate::timing::has("first-draw-dispatch"),
        "Startup did not render",
    )
    .await?;
    pause(200).await;
    std::fs::write(result, crate::timing::report().to_string()).map_err(|error| error.to_string())
}
pub(crate) async fn until(check: impl Fn() -> bool, message: &str) -> Result<(), String> {
    until_with_timeout(check, message, Duration::from_secs(10)).await
}
pub(crate) async fn until_with_timeout(
    check: impl Fn() -> bool,
    message: &str,
    timeout: Duration,
) -> Result<(), String> {
    let started = Instant::now();
    while !check() {
        if started.elapsed() > timeout {
            return Err(message.into());
        }
        pause(5).await;
    }
    Ok(())
}
async fn background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(work());
    });
    let started = Instant::now();
    loop {
        match receive.try_recv() {
            Ok(result) => return result,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("Fixture worker disconnected".into())
            }
            Err(_) if started.elapsed() > Duration::from_secs(15) => {
                return Err("Fixture worker timed out".into())
            }
            Err(_) => pause(5).await,
        }
    }
}
fn dialog(name: &str) -> Option<gtk::Dialog> {
    gtk::Window::list_toplevels()
        .into_iter()
        .filter(|window| window.is_visible() && window.widget_name() == name)
        .find_map(|window| window.downcast().ok())
}
pub(crate) fn field<T: IsA<gtk::Widget> + glib::types::StaticType>(
    root: &impl IsA<gtk::Widget>,
    name: &str,
) -> Result<T, String> {
    let root = root.as_ref();
    if root.widget_name() == name {
        return root
            .clone()
            .downcast::<T>()
            .map_err(|_| format!("Wrong widget type: {name}"));
    }
    if let Some(container) = root.downcast_ref::<gtk::Container>() {
        for child in container.children() {
            if let Ok(found) = field(&child, name) {
                return Ok(found);
            }
        }
    }
    Err(format!("Missing fixture field: {name}"))
}
fn edit_field(dialog: &gtk::Dialog, name: &str, value: &str) -> Result<(), String> {
    field::<gtk::Entry>(dialog, name)?.set_text(value);
    Ok(())
}
pub(crate) fn snapshot(client: &Client, suffix: &str) -> Result<(), String> {
    let Some(path) = std::env::var_os("HSHELL_NATIVE_SNAPSHOT") else {
        return Ok(());
    };
    let path = PathBuf::from(path).with_extension(format!("{suffix}.png"));
    client.window.check_resize();
    let width = client.window.allocated_width();
    let height = client.window.allocated_height();
    let surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, width, height)
        .map_err(|e| e.to_string())?;
    let context = gtk::cairo::Context::new(&surface).map_err(|e| e.to_string())?;
    client.window.draw(&context);
    gdk::pixbuf_get_from_surface(&surface, 0, 0, width, height)
        .ok_or("No native rendered image")?
        .savev(path, "png", &[])
        .map_err(|e| e.to_string())
}
pub(crate) async fn stage(client: &Client, phase: &str, result: &PathBuf) -> Result<(), String> {
    std::fs::write(
        result,
        json!({"phase":phase,"sessions":client.tabs.borrow().len()}).to_string(),
    )
    .map_err(|e| e.to_string())?;
    pause(2500).await;
    Ok(())
}
async fn create_profile(client: &Rc<Client>, credentials: bool) -> Result<Profile, String> {
    client.editor(None);
    let editor = dialog("hshell-profile-editor").ok_or("No profile editor")?;
    editor.response(gtk::ResponseType::Accept);
    if field::<gtk::Label>(&editor, "profile-error")?
        .text()
        .is_empty()
    {
        return Err("Invalid profile accepted".into());
    }
    edit_field(&editor, "profile-name", "开发服务器")?;
    edit_field(&editor, "profile-host", "fixture")?;
    edit_field(&editor, "profile-username", "fixture")?;
    if credentials {
        edit_field(&editor, "profile-password", "fixture-only")?;
    }
    editor.response(gtk::ResponseType::Accept);
    until(
        || dialog("hshell-profile-editor").is_none(),
        "Profile save failed",
    )
    .await?;
    let profile = client
        .profiles
        .borrow()
        .first()
        .cloned()
        .ok_or("Profile not saved")?;
    if profile.password_saved != credentials {
        return Err("Password metadata mismatch".into());
    }
    Ok(profile)
}
pub(crate) async fn connect(
    client: &Rc<Client>,
    base: &Profile,
    index: usize,
) -> Result<(), String> {
    let mut profile = base.clone();
    profile.name = format!("开发服务器 {}", index + 1);
    client.connect_profile(profile);
    let tab = client
        .tabs
        .borrow()
        .last()
        .cloned()
        .ok_or("No native terminal")?;
    until(
        || tab.phase.get() == Phase::Running && tab.terminal.text().contains("fixture-ready"),
        "SSH fixture did not authenticate",
    )
    .await
}
async fn edit_credentials(client: &Rc<Client>, profile: &Profile) -> Result<(), String> {
    for (secret, remember, expected) in [
        ("", true, "fixture-only"),
        ("fixture-replaced", true, "fixture-replaced"),
        ("", false, ""),
        ("fixture-only", true, "fixture-only"),
    ] {
        let current = client
            .profiles
            .borrow()
            .first()
            .cloned()
            .ok_or("Profile missing")?;
        client.editor(Some(current));
        let editor = dialog("hshell-profile-editor").ok_or("Editor missing")?;
        edit_field(&editor, "profile-name", "编辑后的服务器")?;
        edit_field(&editor, "profile-password", secret)?;
        field::<gtk::CheckButton>(&editor, "profile-remember")?.set_active(remember);
        editor.response(gtk::ResponseType::Accept);
        until(
            || dialog("hshell-profile-editor").is_none(),
            "Credential edit failed",
        )
        .await?;
        let current = client
            .profiles
            .borrow()
            .first()
            .cloned()
            .ok_or("Edited profile missing")?;
        let expected = expected.to_string();
        let id = profile.id.clone();
        background(move || {
            if expected.is_empty() {
                if SystemCredentials.get(&id)?.is_some() {
                    return Err("Cleared credential survived".into());
                }
            } else if ssh_core::password_for(&SystemCredentials, &current)?.as_str() != expected {
                return Err("Credential mismatch".into());
            }
            Ok(())
        })
        .await?;
    }
    Ok(())
}
pub(crate) fn answer_confirmation(
    response: gtk::ResponseType,
    repeat_close: Option<gtk::ApplicationWindow>,
) -> Rc<Cell<bool>> {
    let answered = Rc::new(Cell::new(false));
    let done = answered.clone();
    glib::timeout_add_local(Duration::from_millis(10), move || {
        if let Some(dialog) = dialog("hshell-confirmation") {
            if let Some(window) = &repeat_close {
                window.close();
                window.close();
                let done = done.clone();
                glib::timeout_add_local_once(Duration::from_millis(30), move || {
                    dialog.response(response);
                    done.set(true);
                });
            } else {
                dialog.response(response);
                done.set(true);
            }
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
    answered
}
pub fn run(client: Rc<Client>) {
    let app = client.window.application().expect("test application");
    let hold = app.hold();
    glib::MainContext::default().spawn_local(async move {
        let result =
            PathBuf::from(std::env::var_os("HSHELL_NATIVE_RESULT").expect("probe result path"));
        let measured = if std::env::var_os("HSHELL_NATIVE_SFTP").is_some() {
            crate::files::verify(&client, &result).await
        } else if std::env::var_os("HSHELL_NATIVE_PROGRAMS").is_some() {
            crate::program_probe::measure(&client, &result).await
        } else if std::env::var_os("HSHELL_NATIVE_IME").is_some() {
            crate::input_probe::measure(&client, &result).await
        } else if std::env::var_os("HSHELL_NATIVE_STARTUP").is_some() {
            startup(&client, &result).await
        } else if std::env::var_os("HSHELL_NATIVE_HOST_UPDATES").is_some() {
            match until(
                || {
                    client.profiles.borrow().len() == 1000
                        && (std::env::var_os("HSHELL_NATIVE_HOST_INTERLEAVE").is_some()
                            || client.hosts_ready())
                },
                "Host fixture did not load",
            )
            .await
            {
                Ok(()) => match client.benchmark_host_updates().await {
                    Ok(measured) => {
                        std::fs::write(&result, measured.to_string()).map_err(|e| e.to_string())
                    }
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        } else if std::env::var_os("HSHELL_NATIVE_HOST_BENCH").is_some() {
            until(
                || client.profiles.borrow().len() == 1000 && client.hosts_ready(),
                "Host fixture did not load",
            )
            .await
            .and_then(|_| {
                let measured = client.benchmark_hosts()?;
                std::fs::write(&result, measured.to_string()).map_err(|e| e.to_string())
            })
        } else if std::env::var_os("HSHELL_NATIVE_STRESS").is_some() {
            crate::stress::measure(&client, &result).await
        } else {
            measure(&client, &result).await
        };
        if let Err(error) = measured {
            let _ = std::fs::write(&result, json!({"phase":"error","error":error}).to_string());
        }
        client.shutdown();
        drop(hold);
        app.quit();
    });
}
async fn measure(client: &Rc<Client>, result: &PathBuf) -> Result<(), String> {
    let credentials = std::env::var_os("HSHELL_NATIVE_CREDENTIALS").is_some();
    let trust_seen = Rc::new(Cell::new(0));
    let trust_count = trust_seen.clone();
    let auth_error = Rc::new(Cell::new(false));
    let error_flag = auth_error.clone();
    let auth_timer = glib::timeout_add_local(Duration::from_millis(10), move || {
        if let Some(auth) = dialog("hshell-authentication") {
            if auth.title().unwrap_or_default() == "确认主机指纹" {
                let prompt = auth
                    .content_area()
                    .children()
                    .into_iter()
                    .filter_map(|widget| widget.downcast::<gtk::Label>().ok())
                    .map(|label| label.text().to_string())
                    .collect::<String>();
                let expected = std::env::var("HSHELL_NATIVE_FINGERPRINT").unwrap_or_default();
                if expected.is_empty() || !prompt.contains(&expected) {
                    error_flag.set(true);
                    auth.response(gtk::ResponseType::Cancel);
                } else {
                    trust_count.set(trust_count.get() + 1);
                    auth.response(gtk::ResponseType::Accept);
                }
            } else {
                error_flag.set(true);
                auth.response(gtk::ResponseType::Cancel);
            }
        }
        glib::ControlFlow::Continue
    });
    pause(800).await;
    stage(client, "idle", result).await?;
    let profile = create_profile(client, credentials).await?;
    connect(client, &profile, 0).await?;
    stage(client, "one-session", result).await?;
    let first = client.active().ok_or("No active terminal")?;
    let mut input = Vec::new();
    for index in 0..24 {
        let marker = format!("K{index:02}");
        let started = Instant::now();
        first.terminal.send(&marker);
        until(
            || first.terminal.text().contains(&marker),
            "Native key echo timeout",
        )
        .await?;
        input.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    first.terminal.send("\nprobe-fill\n");
    until(
        || first.terminal.text().contains("buffer-filled"),
        "Native scrollback fill timeout",
    )
    .await?;
    if !first.terminal.find("中文滚动", false) {
        return Err("Native Unicode search failed".into());
    }
    let search_scroll = first
        .terminal
        .widget
        .clone()
        .dynamic_cast::<gtk::Scrollable>()
        .map_err(|_| "No search scroll interface")?
        .vadjustment()
        .ok_or("No search adjustment")?;
    pause(30).await;
    let search_start = search_scroll.value();
    // Search navigation completes through GTK events, like separate user clicks.
    for _ in 0..100 {
        if !first.terminal.find("中文滚动", false) {
            return Err("Repeated next match failed".into());
        }
        pause(2).await;
    }
    pause(30).await;
    let search_next = search_scroll.value();
    for _ in 0..100 {
        if !first.terminal.find("中文滚动", true) {
            return Err("Repeated previous match failed".into());
        }
        pause(2).await;
    }
    pause(30).await;
    if search_start == search_next || search_next == search_scroll.value() {
        return Err(format!(
            "Repeated search did not navigate scrollback: {search_start} -> {search_next} -> {}",
            search_scroll.value()
        ));
    }
    if first.terminal.find("no.such[fixture]", false) {
        return Err("Literal search incorrectly matched".into());
    }
    drop(search_scroll);
    first.terminal.find("", false);
    for index in 1..4 {
        connect(client, &profile, index).await?;
    }
    for tab in client.tabs.borrow().iter() {
        tab.terminal.send("probe-fill\n");
    }
    let tabs = client.tabs.borrow().clone();
    for tab in &tabs {
        until(
            || tab.terminal.text().contains("buffer-filled"),
            "Hidden terminal lost output",
        )
        .await?;
    }
    stage(client, "four-filled", result).await?;
    snapshot(client, "full")?;
    let active = client.active().ok_or("No active filled terminal")?;
    let scrollable = active
        .terminal
        .widget
        .clone()
        .dynamic_cast::<gtk::Scrollable>()
        .map_err(|_| "VTE not scrollable")?;
    let adjustment = scrollable.vadjustment().ok_or("No scroll adjustment")?;
    let mut scroll = Vec::new();
    let mut input_under_load = Vec::new();
    active.terminal.send("probe-stream\n");
    for index in 0..80 {
        if index % 8 == 0 {
            let marker = format!("load-echo-{index:02}");
            let started = Instant::now();
            active.terminal.send(&marker);
            until(
                || active.terminal.text().contains(&marker),
                "Input stalled under output",
            )
            .await?;
            input_under_load.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        let started = Instant::now();
        let range = (adjustment.upper() - adjustment.page_size() - adjustment.lower()).max(0.0);
        adjustment.set_value(adjustment.lower() + range * (index % 20) as f64 / 20.0);
        pause(16).await;
        scroll.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    until(
        || active.terminal.text().contains("stream-finished"),
        "Stream fixture did not complete",
    )
    .await?;
    active.terminal.send("\n");
    active.terminal.select_all();
    active.terminal.copy();
    if !gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD)
        .wait_for_text()
        .is_some_and(|text| text.contains("中文持续输出"))
    {
        return Err("Native copy lost Unicode selection".into());
    }
    adjustment.set_value(adjustment.upper() - adjustment.page_size());
    client.window.resize(800, 580);
    pause(300).await;
    active.terminal.send("probe-size\n");
    until(
        || active.terminal.text().contains("fixture-size:"),
        "PTY resize missing",
    )
    .await?;
    let small = active
        .terminal
        .text()
        .lines()
        .find(|line| line.trim_start().starts_with("fixture-size:"))
        .unwrap_or_default()
        .to_string();
    snapshot(client, "small")?;
    client.window.resize(1280, 820);
    pause(300).await;
    active.terminal.send("probe-size\n");
    until(
        || {
            active
                .terminal
                .text()
                .lines()
                .rfind(|line| line.trim_start().starts_with("fixture-size:"))
                .is_some_and(|line| line != small)
        },
        "PTY resize unchanged",
    )
    .await?;
    let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
    clipboard.set_text("paste-cancelled-a\npaste-cancelled-b");
    let answered = answer_confirmation(gtk::ResponseType::Cancel, None);
    client.paste(&active, false);
    until(|| answered.get(), "Multiline paste did not confirm").await?;
    pause(100).await;
    if active.terminal.text().contains("paste-cancelled") {
        return Err("Cancelled paste reached server".into());
    }
    clipboard.set_text("paste-accepted-a\npaste-accepted-b\n");
    let answered = answer_confirmation(gtk::ResponseType::Accept, None);
    client.paste(&active, false);
    until(
        || answered.get() && active.terminal.text().contains("paste-accepted-b"),
        "Accepted paste missing",
    )
    .await?;
    for tab in tabs.iter().take(3) {
        client.close_tab(&tab.id, false);
    }
    drop(tabs);
    drop(first);
    drop(active);
    // The scroll interface also owns a Widget reference. Release the test-only
    // handle before measuring closed terminals, just as the real UI does.
    drop(adjustment);
    drop(scrollable);
    stage(client, "one-after-close", result).await?;
    for index in 4..12 {
        connect(client, &profile, index).await?;
        let id = client.tabs.borrow().last().unwrap().id.clone();
        client.close_tab(&id, false);
    }
    stage(client, "one-after-cycles", result).await?;
    let ids: Vec<_> = client
        .tabs
        .borrow()
        .iter()
        .map(|tab| tab.id.clone())
        .collect();
    for id in ids {
        client.close_tab(&id, false);
    }
    pause(800).await;
    stage(client, "closed-all", result).await?;
    if credentials {
        edit_credentials(client, &profile).await?;
    }
    connect(client, &profile, 12).await?;
    let exited = client.active().ok_or("Missing exit terminal")?;
    exited.terminal.send("probe-exit\n");
    until(
        || exited.phase.get() == Phase::Exited && exited.terminal.text().contains("fixture-exit"),
        "Natural SSH exit lost output",
    )
    .await?;
    if exited.session.borrow().is_some() {
        return Err("Exited SSH session retained backend".into());
    }
    client.close_tab(&exited.id, false);
    connect(client, &profile, 13).await?;
    let answered = answer_confirmation(gtk::ResponseType::Cancel, Some(client.window.clone()));
    client.window.close();
    until(|| answered.get(), "Close confirmation not shown").await?;
    pause(30).await;
    if !answered.get() || client.tabs.borrow().len() != 1 || client.active().is_none() {
        return Err("Close cancellation destroyed session".into());
    }
    let current = client.profiles.borrow().first().cloned();
    client.editor(current);
    let editor = dialog("hshell-profile-editor").ok_or("No delete editor")?;
    let answered = answer_confirmation(gtk::ResponseType::Accept, None);
    editor.response(gtk::ResponseType::Reject);
    until(
        || {
            answered.get()
                && client.profiles.borrow().is_empty()
                && dialog("hshell-profile-editor").is_none()
        },
        "Profile deletion failed",
    )
    .await?;
    if credentials {
        let id = profile.id.clone();
        background(move || {
            if SystemCredentials.get(&id)?.is_some() {
                return Err("Deleted credential survived".into());
            }
            Ok(())
        })
        .await?;
    }
    let destroyed = Rc::new(Cell::new(false));
    let observed = destroyed.clone();
    client.window.connect_destroy(move |_| observed.set(true));
    let answered = answer_confirmation(gtk::ResponseType::Accept, Some(client.window.clone()));
    client.window.close();
    until(|| destroyed.get(), "Accepted close did not destroy window").await?;
    if !answered.get() || !destroyed.get() || !client.tabs.borrow().is_empty() {
        return Err("Window close did not destroy client".into());
    }
    auth_timer.remove();
    if auth_error.get() || (credentials && trust_seen.get() != 1) {
        return Err("Saved password or first-use fingerprint failed".into());
    }
    input.sort_by(f64::total_cmp);
    input_under_load.sort_by(f64::total_cmp);
    scroll.sort_by(f64::total_cmp);
    std::fs::write(result, json!({"phase":"complete","inputP95":input[((input.len()-1) as f64*0.95) as usize],"inputUnderLoadP95":input_under_load[((input_under_load.len()-1) as f64*0.95) as usize],"scrollTickP95":scroll[((scroll.len()-1) as f64*0.95) as usize],"sessions":0,"connections":14,"cycles":8,"unicodeSearch":true,"ptyResize":true,"clipboardConfirmation":true,"unicodeCopy":true,"naturalExit":true,"profileCrud":true,"closeCancelRepeatedAccept":true,"credentials":credentials,"fingerprintConfirmations":trust_seen.get(),"scrollbackLines":3000}).to_string()).map_err(|e|e.to_string())?;
    Ok(())
}
