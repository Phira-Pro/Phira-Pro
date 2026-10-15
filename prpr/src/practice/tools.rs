//! Application-owned analysis scenes use a read-only snapshot and return an optional practice interval.
use crate::{info::ChartInfo, scene::Scene};
#[derive(Clone)]
pub struct Note {
    pub line: u32,
    pub index: u32,
    pub time: f64,
    pub end: f64,
    pub kind: u8,
}
#[derive(Clone)]
pub struct Event {
    pub time: f64,
    pub line: u32,
    pub index: u32,
    pub grade: u8,
    pub diff: f64,
    pub head: bool,
}
pub struct Request {
    pub tool: usize,
    pub local_path: Option<String>,
    pub info: ChartInfo,
    pub key: String,
    pub now: f64,
    pub duration: f64,
    pub notes: Vec<Note>,
    pub events: Vec<Event>,
    pub touches: Vec<TouchSample>,
}
pub struct Selection {
    pub start: f64,
    pub end: f64,
}
#[derive(Clone)]
pub struct TouchSample {
    pub time: f64,
    pub id: u64,
    pub phase: u8,
    pub x: f32,
    pub y: f32,
}
type Factory = fn(Request) -> Box<dyn Scene>;
thread_local! {static FACTORY:std::cell::Cell<Option<Factory>>=const{std::cell::Cell::new(None)};}
pub fn register(factory: Factory) {
    FACTORY.with(|f| f.set(Some(factory)));
}
pub fn open(request: Request) -> Option<Box<dyn Scene>> {
    FACTORY.with(|f| f.get().map(|f| f(request)))
}
