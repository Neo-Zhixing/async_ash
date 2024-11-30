use core::task::ContextBuilder;
use std::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::{atomic::AtomicU64, Arc, Mutex, MutexGuard},
    task::Poll,
};

use ash::{prelude::VkResult, vk};

use crate::{
    command::{states::Recording, CommandBuffer, CommandPool},
    semaphore::TimelineSemaphore,
    Device, HasDevice,
};

use super::{GPUFutureBlock, GPUFutureBlockReturnValue, GPUFutureContext};

fn gpu_future_poll<T: Future>(
    gpu_future: Pin<&mut T>,
    ctx: &mut GPUFutureContext,
) -> Poll<T::Output> {
    use std::task::{RawWaker, RawWakerVTable, Waker};

    fn null_waker_clone_fn(_ptr: *const ()) -> RawWaker {
        panic!("GPU Futures cannot be executed from regular async executors");
    }
    fn null_waker_fn(_ptr: *const ()) {
        panic!("GPU Futures cannot be executed from regular async executors");
    }

    const NULL_WAKER_VTABLE: &'static RawWakerVTable = &RawWakerVTable::new(
        null_waker_clone_fn,
        null_waker_fn,
        null_waker_fn,
        null_waker_fn,
    );
    const NULL_WAKER: &'static Waker = unsafe { &Waker::new(std::ptr::null(), NULL_WAKER_VTABLE) };
    let mut ctx = ContextBuilder::from_waker(NULL_WAKER).ext(ctx).build();
    gpu_future.poll(&mut ctx)
}

pub struct GPUFutureSubmissionStatus<Returned, Retained> {
    return_value: Returned,
    retained_values: Retained,
    timeline_semaphore: Arc<TimelineSemaphore>,
    wait_value: u64,
}

impl CommandPool {
    /// The recorded futures will be executed serially
    pub fn record<T: GPUFutureBlock>(
        &mut self,
        command_buffer: &mut CommandBuffer<Recording>,
        future: T,
    ) -> GPUFutureSubmissionStatus<T::Returned, T::Retained> {
        assert_eq!(command_buffer.pool, self.raw);
        assert_eq!(command_buffer.generation, self.generation);
        let mut future = std::pin::pin!(future);
        let mut stage_count = 0;
        let GPUFutureBlockReturnValue {
            output,
            retained_values,
        } = loop {
            match gpu_future_poll(future.as_mut(), &mut command_buffer.future_ctx) {
                Poll::Ready(output) => break output,
                Poll::Pending => {
                    stage_count += 1;
                    if command_buffer.future_ctx.has_barriers() {
                        // record pipeline barrier
                        unsafe {
                            // Safety: we have mutable borrow to both the command buffer and command pool.
                            self.device().cmd_pipeline_barrier2(
                                command_buffer.raw,
                                &vk::DependencyInfo::default()
                                    .image_memory_barriers(&command_buffer.future_ctx.image_barrier)
                                    .memory_barriers(&[command_buffer.future_ctx.memory_barrier]),
                            );
                        }
                    }
                    command_buffer.future_ctx.clear_barriers();
                }
            }
        };
        GPUFutureSubmissionStatus {
            return_value: output,
            retained_values,
            timeline_semaphore: command_buffer.timeline_semaphore.clone(),
            wait_value: command_buffer.signal_value,
        }
    }
}