use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use ploidy_core::parse::Label;
use serde::Serialize;

/// Statistics for a single generation run.
#[derive(Debug, Serialize)]
pub struct GenerateStats<'a> {
    pub spec: Option<Label<'a>>,
    pub schemas: usize,
    pub operations: BTreeMap<String, usize>,
    pub timings: Timings,
    pub output: OutputStats,
}

/// Wall-clock durations, in seconds, for each generation phase.
#[derive(Debug, Default, Serialize)]
pub struct Timings {
    pub parse: f64,
    pub ir: f64,
    pub cook: f64,
    pub codegen: f64,
}

/// The number of files written, and their combined size in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct OutputStats {
    pub files: usize,
    pub size: usize,
}

#[inline]
pub fn timed<T>(f: impl FnOnce() -> T) -> TimedResult<T> {
    let start = Instant::now();
    let result = f();
    TimedResult(result, start.elapsed())
}

#[derive(Debug)]
pub struct TimedResult<T>(T, Duration);

impl<T> TimedResult<T> {
    #[inline]
    pub fn as_secs_f64(&self) -> f64 {
        self.1.as_secs_f64()
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}
