//! Development-only startup milestones; contains no host names or credentials.
use std::{
    sync::{Mutex, OnceLock},
    time::Instant,
};

static START: OnceLock<Instant> = OnceLock::new();
static MARKS: Mutex<Vec<(&'static str, f64)>> = Mutex::new(Vec::new());

pub fn mark(name: &'static str) {
    let elapsed = START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0;
    let mut marks = MARKS.lock().unwrap();
    if !marks.iter().any(|(key, _)| *key == name) {
        marks.push((name, elapsed));
    }
}

pub fn has(name: &str) -> bool {
    MARKS.lock().unwrap().iter().any(|(key, _)| *key == name)
}

pub fn report() -> serde_json::Value {
    let marks: serde_json::Map<_, _> = MARKS
        .lock()
        .unwrap()
        .iter()
        .map(|(key, value)| ((*key).into(), serde_json::json!(value)))
        .collect();
    serde_json::json!({"phase":"complete","milestonesMs":marks,"notes":"Elapsed from Rust main entry. First draw is GTK draw dispatch, not physical screen presentation. Release optimization with development-only timestamps."})
}
