//! Bounded in-memory execution metadata. Never stores inputs, outputs or messages.
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize)]
pub struct CallRecord {
    pub trace_id: String,
    pub call_id: u64,
    pub action: String,
    pub owner: String,
    pub parent_action: Option<String>,
    pub depth: usize,
    pub started_at: u64,
    pub elapsed_us: u64,
    pub error_code: Option<String>,
}
#[derive(Default)]
pub struct Diagnostics(Mutex<VecDeque<CallRecord>>);
impl Diagnostics {
    pub fn records(&self) -> Vec<CallRecord> {
        self.0.lock().unwrap().iter().cloned().collect()
    }
    pub fn observe(&self, mut record: CallRecord) -> Observation<'_> {
        record.started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Observation {
            diagnostics: self,
            record: Some(record),
            start: Instant::now(),
        }
    }
}
pub struct Observation<'a> {
    diagnostics: &'a Diagnostics,
    record: Option<CallRecord>,
    start: Instant,
}
impl Observation<'_> {
    pub fn finish(&mut self, error_code: Option<&str>) {
        if let Some(record) = &mut self.record {
            record.error_code = error_code.map(|s| s.chars().take(128).collect());
        }
    }
}
impl Drop for Observation<'_> {
    fn drop(&mut self) {
        if let Some(mut record) = self.record.take() {
            record.elapsed_us = self.start.elapsed().as_micros().min(u64::MAX as u128) as u64;
            if std::thread::panicking() {
                record.error_code = Some("plugin_panic".into());
            }
            let mut records = self.diagnostics.0.lock().unwrap();
            if records.len() == 256 {
                records.pop_front();
            }
            records.push_back(record);
        }
    }
}
