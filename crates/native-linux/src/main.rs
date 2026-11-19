#[cfg(target_os = "linux")]
mod files;
#[cfg(all(target_os = "linux", feature = "native-probe"))]
mod input_probe;
#[cfg(all(target_os = "linux", feature = "native-probe"))]
mod probe;
#[cfg(all(target_os = "linux", feature = "native-probe"))]
mod probe_events;
#[cfg(all(target_os = "linux", feature = "native-probe"))]
mod probe_stats;
#[cfg(all(target_os = "linux", feature = "native-probe"))]
mod program_probe;
#[cfg(target_os = "linux")]
mod reclaim;
#[cfg(target_os = "linux")]
mod snippets;
#[cfg(all(target_os = "linux", feature = "native-probe"))]
mod stress;
#[cfg(target_os = "linux")]
mod terminal;
#[cfg(all(target_os = "linux", feature = "native-probe"))]
mod timing;
#[cfg(target_os = "linux")]
mod ui;

#[cfg(target_os = "linux")]
use gtk::{gio, prelude::*};
#[cfg(target_os = "linux")]
use std::{cell::RefCell, rc::Rc};

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The native frontend targets Linux. Build src-tauri for Windows.");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    #[cfg(feature = "native-probe")]
    timing::mark("entry");
    if let Some(code) = ssh_core::run_askpass_helper() {
        std::process::exit(code);
    }
    if std::env::args().any(|arg| arg == "--version") {
        println!("HShell native {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    gtk::glib::set_prgname(Some("hshell"));
    gtk::glib::set_application_name("HShell");
    let flags = if cfg!(feature = "native-probe") {
        gio::ApplicationFlags::NON_UNIQUE
    } else {
        gio::ApplicationFlags::empty()
    };
    #[cfg(feature = "native-probe")]
    timing::mark("before-application");
    let app = gtk::Application::new(Some(ssh_core::APP_ID), flags);
    #[cfg(feature = "native-probe")]
    timing::mark("application-created");
    let state: Rc<RefCell<Option<Rc<ui::Client>>>> = Rc::new(RefCell::new(None));
    let active = state.clone();
    app.connect_activate(move |app| {
        #[cfg(feature = "native-probe")]
        timing::mark("activated");
        if let Some(client) = active.borrow().as_ref() {
            client.window.present();
            return;
        }
        let config = match ssh_core::prepare_app_directory(&gtk::glib::user_config_dir()) {
            Ok(config) => config,
            Err(error) => {
                let dialog = gtk::MessageDialog::new(
                    None::<&gtk::Window>,
                    gtk::DialogFlags::MODAL,
                    gtk::MessageType::Error,
                    gtk::ButtonsType::Close,
                    "无法打开 HShell",
                );
                dialog.set_secondary_text(Some(&error));
                dialog.run();
                dialog.close();
                app.quit();
                return;
            }
        };
        let client = ui::Client::new(app, config.join("profiles.json"));
        #[cfg(feature = "native-probe")]
        if std::env::var_os("HSHELL_NATIVE_PROBE").is_some() {
            probe::run(client.clone());
        }
        *active.borrow_mut() = Some(client);
    });
    app.connect_shutdown(move |_| {
        if let Some(client) = state.borrow_mut().take() {
            client.shutdown();
        }
    });
    app.run_with_args::<&str>(&[]);
    hshell_sftp::wait_for_shutdown(std::time::Duration::from_secs(4));
}
