//! Development-only real SSH stress path; absent from production builds.
use crate::{
    probe::{connect, pause, stage, until},
    probe_stats::Histogram,
    ui::Client,
};
use gtk::{glib, prelude::*};
use serde_json::json;
use ssh_core::Profile;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

pub async fn measure(client: &Rc<Client>, result: &PathBuf) -> Result<(), String> {
    let count: usize = std::env::var("HSHELL_STRESS_TABS")
        .unwrap()
        .parse()
        .map_err(|_| "Invalid tab count")?;
    let seconds: u64 = std::env::var("HSHELL_STRESS_SECONDS")
        .unwrap()
        .parse()
        .map_err(|_| "Invalid duration")?;
    let cycles: usize = std::env::var("HSHELL_STRESS_CYCLES")
        .unwrap()
        .parse()
        .map_err(|_| "Invalid cycles")?;
    if !(2..=12).contains(&count)
        || !(5..=28_800).contains(&seconds)
        || !(1..=100).contains(&cycles)
    {
        return Err("Stress fixture limits exceeded".into());
    }
    pause(800).await;
    stage(client, "stress-idle", result).await?;
    let profile = Profile {
        id: "stress-fixture".into(),
        name: "压力测试".into(),
        host: "fixture".into(),
        username: "fixture".into(),
        port: None,
        identity_file: String::new(),
        group: String::new(),
        favorite: false,
        password_saved: false,
    };
    for index in 0..count {
        connect(client, &profile, index).await?;
    }
    let tabs = client.tabs.borrow().clone();
    let active = tabs.last().unwrap();
    let scrollable = active
        .terminal
        .widget
        .clone()
        .dynamic_cast::<gtk::Scrollable>()
        .map_err(|_| "No scroll interface")?;
    let adjustment = scrollable.vadjustment().ok_or("No scroll adjustment")?;
    let ticks = Rc::new(RefCell::new(Histogram::default()));
    let tick_samples = ticks.clone();
    let previous = Rc::new(Cell::new(Instant::now()));
    let tick = glib::timeout_add_local(Duration::from_millis(16), move || {
        tick_samples
            .borrow_mut()
            .record(previous.replace(Instant::now()).elapsed().as_secs_f64() * 1000.0);
        glib::ControlFlow::Continue
    });
    for tab in &tabs {
        tab.terminal.send(&format!("probe-stress:{seconds}\n"));
    }
    std::fs::write(
        result,
        json!({"phase":"stress-streaming","sessions":count}).to_string(),
    )
    .map_err(|e| e.to_string())?;
    let mut input = Histogram::default();
    let mut partial_input = Histogram::default();
    let partial_load = std::env::var_os("HSHELL_STRESS_PARTIAL_LOAD").is_some();
    let profiling = std::env::var_os("HSHELL_NATIVE_PROFILE").is_some();
    if profiling {
        // Under instruction instrumentation, frequent text extraction would
        // distort the output parser profile. Latency is measured uninstrumented.
        pause(seconds * 1000).await;
    }
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut index = 0;
    while !profiling && Instant::now() < deadline {
        let marker = format!("stress-key-{index:04}");
        let started = Instant::now();
        active.terminal.send(&marker);
        until(
            || active.terminal.recent_text(200).contains(&marker),
            "Input stalled during multi-session output",
        )
        .await?;
        input.record(started.elapsed().as_secs_f64() * 1000.0);
        let range = (adjustment.upper() - adjustment.page_size() - adjustment.lower()).max(0.0);
        adjustment.set_value(adjustment.lower() + range * (index % 10) as f64 / 10.0);
        pause(500).await;
        index += 1;
    }
    for tab in &tabs {
        until(
            || {
                tab.terminal
                    .recent_text(200)
                    .contains("stress-stream-finished")
            },
            "Hidden terminal failed to finish streaming",
        )
        .await?;
    }
    tick.remove();
    drop(adjustment);
    drop(scrollable);
    stage(client, "stress-filled", result).await?;
    for tab in tabs.iter().skip(1) {
        client.close_tab(&tab.id, false);
    }
    drop(tabs);
    if partial_load {
        let survivor = client.active().ok_or("Missing surviving terminal")?;
        survivor.terminal.send("probe-stress:5\n");
        for index in 0..30 {
            let marker = format!("partial-key-{index:03}");
            let started = Instant::now();
            survivor.terminal.send(&marker);
            until(
                || survivor.terminal.recent_text(200).contains(&marker),
                "Input stalled during heap reclamation",
            )
            .await?;
            partial_input.record(started.elapsed().as_secs_f64() * 1000.0);
            pause(50).await;
        }
        until(
            || {
                survivor
                    .terminal
                    .recent_text(200)
                    .contains("stress-stream-finished")
            },
            "Surviving terminal stopped streaming",
        )
        .await?;
    }
    pause(1000).await;
    stage(client, "stress-one-after-close", result).await?;
    for index in 0..cycles {
        connect(client, &profile, count + index).await?;
        let tab = client.active().ok_or("Missing cycle tab")?;
        tab.terminal.send("probe-fill\n");
        until(
            || tab.terminal.recent_text(200).contains("buffer-filled"),
            "Cycle fill did not complete",
        )
        .await?;
        client.close_tab(&tab.id, false);
    }
    pause(1000).await;
    stage(client, "stress-one-after-cycles", result).await?;
    let id = client.tabs.borrow()[0].id.clone();
    client.close_tab(&id, false);
    pause(1000).await;
    stage(client, "stress-closed-all", result).await?;
    let ticks = ticks.borrow();
    std::fs::write(result, json!({"phase":"complete","stress":true,"tabs":count,"seconds":seconds,"cycles":cycles,"connections":count+cycles,"sessions":0,"hiddenOutputComplete":true,"inputP95":input.p95(),"inputSamples":input.count(),"partialLoad":partial_load,"partialInputP95":partial_input.p95(),"loopTickP95":ticks.p95(),"loopTickMax":ticks.maximum(),"loopTickSamples":ticks.count(),"histogramResolutionMs":0.25,"scrollbackLines":3000,"reclaimCalls":crate::reclaim::calls()}).to_string()).map_err(|e|e.to_string())?;
    Ok(())
}
