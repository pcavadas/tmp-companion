//! Releasing the Windows transport's always-pending overlapped read, exercised with
//! injected OS outcomes (the pattern of `write_completion`).
//!
//! The kernel writes a pending read's result into the `OVERLAPPED` and the read buffer when
//! it completes, so both must outlive the I/O: a device handle dropped with a read still in
//! flight is a use-after-free (#266). [`release`] is what the device's `Drop` runs, on the
//! normal close and on a worker unwinding alike.

pub(super) trait PendingRead {
    /// A read is in flight.
    fn is_pending(&self) -> bool;
    /// Ask the OS to cancel it (`CancelIoEx`). Only a request: the read may still complete.
    fn cancel(&mut self);
    /// Block until the read reaches terminal completion, cancelled or not.
    fn finish(&mut self);
}

/// Cancel a pending read and wait for it to land, so its storage can be freed.
pub(super) fn release(read: &mut impl PendingRead) {
    if read.is_pending() {
        read.cancel();
        // Always drain, even when the cancel lost a race with a real completion.
        read.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A device whose `Drop` runs [`release`], as the Windows `Dev` does.
    struct FakeDev {
        pending: bool,
        buffer: Box<[u8]>,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }
    impl PendingRead for FakeDev {
        fn is_pending(&self) -> bool {
            self.pending
        }
        fn cancel(&mut self) {
            self.calls.lock().unwrap().push("cancel");
        }
        fn finish(&mut self) {
            // The driver touches the retained buffer when the read actually completes.
            self.buffer[0] = 7;
            self.pending = false;
            self.calls.lock().unwrap().push("finish");
        }
    }
    impl Drop for FakeDev {
        fn drop(&mut self) {
            release(self);
            assert!(!self.pending, "storage released with a read in flight");
            self.calls.lock().unwrap().push("freed");
        }
    }

    #[test]
    fn a_worker_unwinding_with_a_pending_read_cancels_and_waits_before_freeing() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let log = calls.clone();
        let worker = std::thread::spawn(move || {
            let _dev = FakeDev {
                pending: true,
                buffer: vec![0].into_boxed_slice(),
                calls: log,
            };
            panic!("the HID worker unwinds mid-pump");
        });
        assert!(worker.join().is_err());
        assert_eq!(*calls.lock().unwrap(), ["cancel", "finish", "freed"]);
    }

    #[test]
    fn no_pending_read_means_no_cancel() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        drop(FakeDev {
            pending: false,
            buffer: vec![0].into_boxed_slice(),
            calls: calls.clone(),
        });
        assert_eq!(*calls.lock().unwrap(), ["freed"]);
    }
}
