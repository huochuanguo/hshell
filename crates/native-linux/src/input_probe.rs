//! Actual GTK key events through an isolated IBus/libpinyin input context.
use crate::{
    probe::{connect, pause, until},
    probe_events::key,
    ui::Client,
};
use gtk::{gdk, prelude::*};
use ssh_core::Profile;
use std::{path::Path, rc::Rc, time::Instant};

pub async fn measure(client: &Rc<Client>, result: &Path) -> Result<(), String> {
    pause(500).await;
    let profile = Profile {
        id: "ime-fixture".into(),
        name: "输入法检查".into(),
        host: "fixture".into(),
        username: "fixture".into(),
        port: None,
        identity_file: String::new(),
        group: String::new(),
        favorite: false,
        password_saved: false,
    };
    connect(client, &profile, 0).await?;
    let tab = client.active().ok_or("No input terminal")?;
    client.window.present();
    tab.terminal.widget.grab_focus();
    pause(800).await;
    std::fs::write(result, r#"{"phase":"ime-ready","sessions":1}"#)
        .map_err(|error| error.to_string())?;
    until(
        || result.with_extension("ready").exists(),
        "Input engine was not selected",
    )
    .await?;
    pause(500).await;
    let mut elapsed = Vec::new();
    for index in 0..3 {
        for character in "nihao".chars() {
            key(
                &tab.terminal.widget,
                character as u32,
                gdk::ModifierType::empty(),
            )?;
            pause(80).await;
        }
        pause(250).await;
        let before = tab.terminal.recent_text(100);
        if before.matches("你好").count() != index || before.contains("nihao") {
            return Err("Pinyin preedit bypassed the input context".into());
        }
        let started = Instant::now();
        key(&tab.terminal.widget, 0x20, gdk::ModifierType::empty())?;
        until(
            || tab.terminal.recent_text(100).matches("你好").count() == index + 1,
            "IBus/libpinyin did not commit Chinese to SSH",
        )
        .await?;
        elapsed.push(started.elapsed().as_secs_f64() * 1000.0);
        tab.terminal.send("\n");
        pause(100).await;
    }
    client.close_tab(&tab.id, false);
    pause(700).await;
    std::fs::write(result, serde_json::json!({"phase":"complete","connections":1,"ime":"IBus/libpinyin","commits":3,
        "preeditWasNotSentToSsh":true,"commitToParsedEchoMs":elapsed,"sessions":0,
        "notes":"Programmatically delivered GTK key events through real isolated IBus/libpinyin. Not physical keyboard or display latency."}).to_string()).map_err(|error|error.to_string())
}
