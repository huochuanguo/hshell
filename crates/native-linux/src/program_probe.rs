//! 使用真实 SSH 验证 Bash 补全、Vim、tmux 和 GTK 输入，仅用于开发。
use crate::{
    probe::{connect, pause, until},
    ui::{Client, Phase},
};
use gtk::{gdk, prelude::*};
use ssh_core::Profile;
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use crate::probe_events::{key, scroll};
pub async fn measure(client: &Rc<Client>, result: &Path) -> Result<(), String> {
    let root =
        PathBuf::from(std::env::var_os("HSHELL_PROGRAM_ROOT").ok_or("Program root missing")?);
    let profile = Profile {
        id: "program-fixture".into(),
        name: "终端程序检查".into(),
        host: "fixture".into(),
        username: "root".into(),
        port: None,
        identity_file: String::new(),
        group: String::new(),
        favorite: false,
        password_saved: false,
    };
    connect(client, &profile, 0).await?;
    let tab = client.active().ok_or("Program terminal missing")?;
    client.window.present();
    tab.terminal.widget.grab_focus();
    pause(300).await;
    until(
        || tab.terminal.recent_text(5).contains("fixture-ready>"),
        "Bash did not present an interactive prompt",
    )
    .await?;
    for (prefix, name, marker, physical_key) in [
        ("tab-f", "tab-file.txt", "TAB_FILE_CONTENT", true),
        ("button-f", "button-file.txt", "BUTTON_FILE_CONTENT", false),
    ] {
        tab.terminal.send(&format!("cat /fixture/{prefix}"));
        if physical_key {
            key(
                &tab.terminal.widget,
                *gdk::keys::constants::Tab,
                gdk::ModifierType::empty(),
            )?;
        } else {
            if !client.tab_key.is_sensitive() {
                return Err("Tab toolbar action unavailable during SSH session".into());
            }
            client.tab_key.grab_focus();
            client.tab_key.emit_clicked();
            if !tab.terminal.widget.is_focus() {
                return Err("Tab toolbar action did not restore terminal focus".into());
            }
        }
        let completed = format!("cat /fixture/{name}");
        until(
            || tab.terminal.recent_text(5).contains(&completed),
            "Tab did not complete a remote Bash filename",
        )
        .await?;
        if tab.terminal.recent_text(5).contains(marker) {
            return Err("Tab executed the command before Enter".into());
        }
        key(
            &tab.terminal.widget,
            *gdk::keys::constants::Return,
            gdk::ModifierType::empty(),
        )?;
        until(
            || tab.terminal.recent_text(5).contains(marker),
            "The completed command did not run when Enter was pressed",
        )
        .await?;
    }
    let command = "printf ok > /fixture/snippet-output";
    if !tab.terminal.insert_snippet(command) {
        return Err("Single-line snippet was rejected".into());
    }
    until(
        || tab.terminal.recent_text(5).contains(command),
        "Snippet did not reach Bash",
    )
    .await?;
    pause(100).await;
    let output = root.join("snippet-output");
    if output.exists() || tab.terminal.insert_snippet("pwd\n") {
        return Err("Snippet insertion executed or accepted an Enter key".into());
    }
    key(
        &tab.terminal.widget,
        *gdk::keys::constants::Return,
        gdk::ModifierType::empty(),
    )?;
    until(
        || std::fs::read_to_string(&output).is_ok_and(|text| text == "ok"),
        "Inserted snippet did not run on Enter",
    )
    .await?;
    tab.terminal
        .send("tmux -L hshell -f /dev/null new-session -s fixture\r");
    until(
        || tab.terminal.screen_text().contains("[fixture]"),
        "tmux did not start",
    )
    .await?;
    key(
        &tab.terminal.widget,
        'b' as u32,
        gdk::ModifierType::CONTROL_MASK,
    )?;
    pause(100).await;
    key(&tab.terminal.widget, 'c' as u32, gdk::ModifierType::empty())?;
    pause(400).await;
    tab.terminal
        .send("tmux -L hshell list-windows -F '#{window_index}' > /fixture/tmux-windows\r");
    until(
        || {
            std::fs::read_to_string(root.join("tmux-windows"))
                .is_ok_and(|text| text.lines().count() == 2)
        },
        "Ctrl B c did not open a second tmux window",
    )
    .await?;
    tab.terminal
        .send("vim.tiny -Nu NONE -n /fixture/vim-check.txt\r");
    until(
        || {
            let screen = tab.terminal.screen_text();
            screen.contains("vim-check.txt") && screen.contains('~')
        },
        "vim did not enter alternate screen",
    )
    .await?;
    key(&tab.terminal.widget, 'i' as u32, gdk::ModifierType::empty())?;
    pause(100).await;
    let content = "HShell 中文输入与保存 UTF-8";
    tab.terminal.paste(content);
    pause(200).await;
    for (width, height) in [(900, 580), (1280, 780), (1040, 680)] {
        client.window.resize(width, height);
        pause(200).await;
    }
    until(
        || tab.terminal.screen_text().contains(content),
        "vim lost Unicode content during resize",
    )
    .await?;
    key(&tab.terminal.widget, 0xff1b, gdk::ModifierType::empty())?;
    pause(100).await;
    tab.terminal.send(":wq\r");
    until(
        || {
            std::fs::read_to_string(root.join("vim-check.txt"))
                .is_ok_and(|text| text == format!("{content}\n"))
        },
        "vim did not save exact UTF-8 bytes",
    )
    .await?;
    tab.terminal.send("tmux -L hshell kill-server\r");
    pause(300).await;
    tab.terminal.send("for i in $(seq 1 4000); do printf 'scroll-line-%s 中文\\n' \"$i\"; done; printf 'scroll-ready\\n'\r");
    until(
        || tab.terminal.recent_text(5).contains("scroll-ready"),
        "Scroll output did not finish",
    )
    .await?;
    pause(200).await;
    let adjustment = tab
        .terminal
        .widget
        .property::<gtk::Adjustment>("vadjustment");
    let bottom = adjustment.value();
    for _ in 0..80 {
        scroll(&tab.terminal.widget, false)?;
        pause(2).await;
    }
    pause(150).await;
    let scrolled = adjustment.value();
    if bottom - scrolled < 100.0 {
        return Err(format!(
            "Rapid scroll did not move viewport: {bottom} -> {scrolled}"
        ));
    }
    for _ in 0..80 {
        scroll(&tab.terminal.widget, true)?;
        pause(2).await;
    }
    key(&tab.terminal.widget, 'e' as u32, gdk::ModifierType::empty())?;
    tab.terminal
        .send("cho after-scroll-ok; stty size > /fixture/terminal-size\r");
    until(
        || {
            root.join("terminal-size").exists()
                && tab.terminal.recent_text(10).contains("after-scroll-ok")
        },
        "Input failed after rapid scroll",
    )
    .await?;
    tab.terminal.send("exit\r");
    until(
        || tab.phase.get() == Phase::Exited,
        "SSH did not exit naturally",
    )
    .await?;
    client.close_tab(&tab.id, false);
    pause(700).await;
    std::fs::write(result,serde_json::json!({"phase":"complete","sessions":0,"connections":1,"bashTabCompletion":true,"snippetInsertOnly":true,"tmuxWindows":2,"vimUtf8Saved":true,"resizeCount":3,"scrollEvents":160,"scrollRowsMoved":bottom-scrolled,"inputAfterScroll":true,"naturalExit":true,"notes":"Real remote vim.tiny/tmux over OpenSSH; programmatic GTK key and wheel events. Unicode paste is distinct from IME composition."}).to_string()).map_err(|e|e.to_string())
}
