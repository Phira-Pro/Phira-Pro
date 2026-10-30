//! Native extension contract; script/plugin adapters can implement this later.
use super::{presentation::PresentationAssets, FrameContext, RenderTimeline, VideoOptions};
use crate::ui::Ui;
use anyhow::Result;
use macroquad::prelude::Color;

pub const RENDER_API_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderLayer {
    Background,
    BeforeScene,
    AfterScene,
    AfterUi,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderOutcome {
    Completed,
    Cancelled,
    Failed,
}

/// Per-frame visual decisions, reset before every frame, including previews.
pub struct FrameBehavior {
    pub opacity: f32,
    pub tint: Color,
}

impl Default for FrameBehavior {
    fn default() -> Self {
        Self {
            opacity: 1.,
            tint: macroquad::prelude::WHITE,
        }
    }
}

/// Extensions are owned by one export. No process-wide registry or shared UI state.
/// Timing changes happen only in configure, before validation and timeline creation.
pub trait RenderExtension {
    fn id(&self) -> &str;
    fn configure(&mut self, _options: &mut VideoOptions) -> Result<()> {
        Ok(())
    }
    fn prepare(&mut self, _timeline: &RenderTimeline, _assets: &PresentationAssets) -> Result<()> {
        Ok(())
    }
    fn stage_enter(&mut self, _context: &FrameContext) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, _context: &FrameContext, _behavior: &mut FrameBehavior) -> Result<()> {
        Ok(())
    }
    fn draw(&mut self, _layer: RenderLayer, _context: &FrameContext, _ui: &mut Ui) -> Result<()> {
        Ok(())
    }
    /// Audio chunks use the movie clock, stereo floating-point samples at 44100 Hz.
    fn audio(&mut self, _first_sample: u64, _samples: &mut [sasa::Frame]) -> Result<()> {
        Ok(())
    }
    fn finish(&mut self, _outcome: RenderOutcome) {}
}

/// Owns lifecycle notification even when loading fails or its future is dropped.
pub(super) struct ExtensionSet {
    pub items: Vec<Box<dyn RenderExtension>>,
    outcome: Option<RenderOutcome>,
}

impl ExtensionSet {
    pub fn new(items: Vec<Box<dyn RenderExtension>>) -> Self {
        Self { items, outcome: None }
    }
    pub fn finish(&mut self, outcome: RenderOutcome) {
        if self.outcome.is_none() {
            self.outcome = Some(outcome);
            for item in &mut self.items {
                item.finish(outcome);
            }
        }
    }
}

impl Drop for ExtensionSet {
    fn drop(&mut self) {
        self.finish(RenderOutcome::Failed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    struct Observer(Rc<RefCell<Vec<RenderOutcome>>>);
    impl RenderExtension for Observer {
        fn id(&self) -> &str {
            "example.observer"
        }
        fn finish(&mut self, outcome: RenderOutcome) {
            self.0.borrow_mut().push(outcome);
        }
    }
    #[test]
    fn unfinished_loading_and_explicit_outcomes_notify_exactly_once() {
        for outcome in [None, Some(RenderOutcome::Cancelled), Some(RenderOutcome::Completed)] {
            let events = Rc::new(RefCell::new(vec![]));
            {
                let mut set = ExtensionSet::new(vec![Box::new(Observer(events.clone()))]);
                if let Some(outcome) = outcome {
                    set.finish(outcome);
                    set.finish(RenderOutcome::Failed);
                }
            }
            assert_eq!(*events.borrow(), vec![outcome.unwrap_or(RenderOutcome::Failed)]);
        }
    }
}
