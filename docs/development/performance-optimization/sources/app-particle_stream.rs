//! A bounded research particle stream and its independent deletion ownership.
//! The existing full particle/geometry/pipeline lifetime is unchanged.

pub(crate) const SMALL_STREAM_INSTANCES: usize = 256;
pub(crate) const SMALL_STREAM_CAPACITY: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UploadDecision {
    Empty,
    Off,
    Missing { bytes: usize },
    Oversize { bytes: usize },
    Attempt { bytes: usize },
}

/// Decide before touching a GL binding. A multiplication overflow is an
/// oversize batch with no representable byte count, never a small upload.
pub(crate) fn upload_decision(requested: bool, instances: usize, stride: usize, present: bool) -> UploadDecision {
    if instances == 0 {
        return UploadDecision::Empty;
    }
    if !requested {
        return UploadDecision::Off;
    }
    let bytes = match instances.checked_mul(stride) {
        Some(bytes) if bytes != 0 => bytes,
        _ => return UploadDecision::Oversize { bytes: 0 },
    };
    if instances > SMALL_STREAM_INSTANCES || bytes > SMALL_STREAM_CAPACITY {
        UploadDecision::Oversize { bytes }
    } else if !present {
        UploadDecision::Missing { bytes }
    } else {
        UploadDecision::Attempt { bytes }
    }
}

/// Each queued record can yield its resource at most once. Foreign records are
/// consumed without deletion: a managed identity is not an EGL loss detector.
struct Retirement<T> {
    context: usize,
    resource: Option<T>,
}

impl<T> Retirement<T> {
    fn new(context: usize, resource: T) -> Self {
        Self { context, resource: Some(resource) }
    }

    fn take_for_context(&mut self, context: usize) -> Option<T> {
        let resource = self.resource.take()?;
        (self.context == context).then_some(resource)
    }
}

// Keeping the pure decisions above independent of GL also permits a direct
// std-only `rustc --test particle_stream.rs` boundary/retirement test run.
#[cfg(feature = "render_research")]
mod gpu {
    use super::{Retirement, SMALL_STREAM_CAPACITY};
    use macroquad::{
        prelude::get_internal_gl,
        window::miniquad::{gl, Buffer, BufferType, Context},
    };
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RetirementState {
        pending: Vec<Retirement<Buffer>>,
        created_owners: u64,
        queued: u64,
        deleted: u64,
        foreign: u64,
    }

    static RETIRED: Lazy<Mutex<RetirementState>> = Lazy::new(|| Mutex::new(RetirementState::default()));

    /// Single deletion owner. Bindings may copy the raw miniquad handle, but
    /// they never acquire deletion ownership. Drop only enqueues a record;
    /// the global mutex also makes an off-thread drop safe without raw GL.
    pub(crate) struct OwnedSmallStream {
        retirement: Option<Retirement<Buffer>>,
    }

    impl OwnedSmallStream {
        pub(crate) fn new(context: &mut Context, force_missing: bool) -> Option<Self> {
            if force_missing || !gl::is_function_loaded("glGetError") {
                return None;
            }
            // Loading only. Preserve evidence of an earlier operation's error
            // in the log and decline rather than attributing it to this buffer.
            let prior_error = unsafe { gl::glGetError() };
            if prior_error != 0 {
                tracing::warn!("Particle small stream not created: pre-existing GL error 0x{prior_error:x}");
                return None;
            }
            let buffer = Buffer::stream(context, BufferType::VertexBuffer, SMALL_STREAM_CAPACITY);
            let owner = Self {
                retirement: Some(Retirement::new(context.context_id(), buffer)),
            };
            {
                let mut state = RETIRED.lock().unwrap();
                state.created_owners = state.created_owners.saturating_add(1);
            }
            let creation_error = unsafe { gl::glGetError() };
            if creation_error != 0 {
                tracing::warn!("Particle small stream allocation failed: GL error 0x{creation_error:x}");
                // This newly created name still has a deletion owner. Its Drop
                // queues it for the regular render-thread retirement hook.
                return None;
            }
            Some(owner)
        }

        pub(crate) fn buffer_for_context(&self, context: usize) -> Option<Buffer> {
            let retirement = self.retirement.as_ref()?;
            if retirement.context != context {
                return None;
            }
            retirement.resource
        }
    }

    impl Drop for OwnedSmallStream {
        fn drop(&mut self) {
            if let Some(retirement) = self.retirement.take() {
                let mut state = RETIRED.lock().unwrap();
                state.queued = state.queued.saturating_add(1);
                state.pending.push(retirement);
            }
        }
    }

    /// Called by the existing main render-thread deletion hook. Deferred
    /// managed draws must consume their bindings before any name is deleted.
    pub(crate) fn flush_pending_deletions() {
        let retired = std::mem::take(&mut RETIRED.lock().unwrap().pending);
        if retired.is_empty() {
            return;
        }
        let mut internal = unsafe { get_internal_gl() };
        internal.flush();
        let context = internal.quad_context;
        let mut deleted = 0_u64;
        let mut foreign = 0_u64;
        for mut retirement in retired {
            if let Some(buffer) = retirement.take_for_context(context.context_id()) {
                context.clear_resource_bindings();
                buffer.delete();
                deleted = deleted.saturating_add(1);
            } else {
                foreign = foreign.saturating_add(1);
            }
        }
        let mut state = RETIRED.lock().unwrap();
        state.deleted = state.deleted.saturating_add(deleted);
        state.foreign = state.foreign.saturating_add(foreign);
    }

    /// Loading/drop/retirement counters, never updated by a stream upload.
    /// Owner creation includes allocation attempts later rejected by GL error;
    /// deletion counts a submitted delete, not physical driver reclamation.
    pub(crate) fn retirement_report() -> (u64, u64, u64, u64, usize) {
        let state = RETIRED.lock().unwrap();
        (state.created_owners, state.queued, state.deleted, state.foreign, state.pending.len())
    }
}

#[cfg(feature = "render_research")]
pub(crate) use gpu::{flush_pending_deletions, retirement_report, OwnedSmallStream};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_batches_never_attempt_an_upload_or_require_a_buffer() {
        for requested in [false, true] {
            for present in [false, true] {
                assert_eq!(upload_decision(requested, 0, usize::MAX, present), UploadDecision::Empty);
            }
        }
    }

    #[test]
    fn one_and_exactly_256_instances_fit_the_fixed_16k_stream() {
        assert_eq!(upload_decision(true, 1, 64, true), UploadDecision::Attempt { bytes: 64 });
        assert_eq!(upload_decision(true, 256, 64, true), UploadDecision::Attempt { bytes: 16_384 });
        assert_eq!(upload_decision(true, 257, 64, true), UploadDecision::Oversize { bytes: 16_448 });
    }

    #[test]
    fn both_instance_and_byte_limits_must_fit() {
        assert_eq!(upload_decision(true, 257, 1, true), UploadDecision::Oversize { bytes: 257 });
        assert_eq!(upload_decision(true, 256, 65, true), UploadDecision::Oversize { bytes: 16_640 });
        assert_eq!(
            upload_decision(true, 1, SMALL_STREAM_CAPACITY + 1, true),
            UploadDecision::Oversize { bytes: SMALL_STREAM_CAPACITY + 1 },
        );
    }

    #[test]
    fn missing_and_oversize_choose_different_fallback_reasons() {
        assert_eq!(upload_decision(true, 1, 64, false), UploadDecision::Missing { bytes: 64 });
        assert_eq!(upload_decision(true, 256, 64, false), UploadDecision::Missing { bytes: 16_384 });
        assert_eq!(upload_decision(true, 257, 64, false), UploadDecision::Oversize { bytes: 16_448 });
    }

    #[test]
    fn unrepresentable_and_zero_sized_batches_cannot_reach_the_api() {
        assert_eq!(upload_decision(true, usize::MAX, 64, true), UploadDecision::Oversize { bytes: 0 });
        assert_eq!(upload_decision(true, 2, usize::MAX, true), UploadDecision::Oversize { bytes: 0 });
        assert_eq!(upload_decision(true, 1, 0, true), UploadDecision::Oversize { bytes: 0 });
    }

    #[test]
    fn off_choice_does_not_depend_on_small_availability_or_range() {
        for present in [false, true] {
            for count in [1, 256, 257, usize::MAX] {
                assert_eq!(upload_decision(false, count, usize::MAX, present), UploadDecision::Off);
            }
        }
    }

    #[test]
    fn small_full_small_selection_is_independent_per_batch() {
        let counts = [1, 257, 256, 0, 1];
        let decisions = counts.map(|count| upload_decision(true, count, 64, true));
        assert_eq!(
            decisions,
            [
                UploadDecision::Attempt { bytes: 64 },
                UploadDecision::Oversize { bytes: 16_448 },
                UploadDecision::Attempt { bytes: 16_384 },
                UploadDecision::Empty,
                UploadDecision::Attempt { bytes: 64 },
            ],
        );
    }

    #[test]
    fn a_matching_retirement_yields_the_owned_name_once() {
        let mut record = Retirement::new(11, 42_u32);
        assert_eq!(record.take_for_context(11), Some(42));
        assert_eq!(record.take_for_context(11), None);
        assert_eq!(record.take_for_context(12), None);
    }

    #[test]
    fn foreign_retirement_never_deletes_a_reused_name_in_a_new_context() {
        let mut old = Retirement::new(11, 42_u32);
        let mut current = Retirement::new(12, 42_u32);
        assert_eq!(old.take_for_context(12), None);
        assert_eq!(old.take_for_context(11), None);
        assert_eq!(current.take_for_context(12), Some(42));
        assert_eq!(current.take_for_context(12), None);
    }
}
