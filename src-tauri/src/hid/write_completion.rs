//! Pending Windows write completion policy, exercised with injected OS outcomes.

/// Failure policy, not a hardware-calibrated latency: after five seconds ask
/// Windows to cancel. Cancellation completion can take longer; its storage must
/// remain valid until terminal completion, even when CancelIoEx fails or races.
pub(super) const WRITE_TIMEOUT_MS: u32 = 5_000;

pub(super) trait PendingWrite {
    /// true = signaled, false = timeout; errors are wait failures.
    fn wait(&mut self, timeout_ms: u32) -> Result<bool, String>;
    fn cancel(&mut self) -> Result<(), String>;
    /// Wait for terminal completion; never release an in-flight buffer here.
    fn finish(&mut self) -> Result<u32, String>;
}

pub(super) fn complete(write: &mut impl PendingWrite) -> Result<u32, String> {
    let failure = match write.wait(WRITE_TIMEOUT_MS) {
        Ok(true) => return write.finish(),
        Ok(false) => format!("HID write timed out after {WRITE_TIMEOUT_MS} ms"),
        Err(e) => e,
    };
    let cancel = write.cancel();
    // CancelIoEx only requests cancellation. Always drain, including a cancel
    // ERROR_NOT_FOUND race with successful completion, before releasing storage.
    let terminal = write.finish();
    Err(format!(
        "{failure}; cancellation: {cancel:?}; completion: {terminal:?}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        buffer: Box<[u8]>,
        wait: Result<bool, String>,
        cancel: Result<(), String>,
        terminal: Result<u32, String>,
        calls: Vec<&'static str>,
        completed: bool,
    }
    impl PendingWrite for Fake {
        fn wait(&mut self, timeout_ms: u32) -> Result<bool, String> {
            assert_eq!(timeout_ms, 5_000);
            self.calls.push("wait");
            self.wait.clone()
        }
        fn cancel(&mut self) -> Result<(), String> {
            assert!(!self.completed);
            assert_eq!(self.buffer[0], 42);
            self.calls.push("cancel");
            self.cancel.clone()
        }
        fn finish(&mut self) -> Result<u32, String> {
            // Simulate the driver accessing retained storage when cancellation
            // actually completes, rather than when it was merely requested.
            assert_eq!(self.buffer[0], 42);
            self.buffer[0] = 7;
            self.calls.push("terminal");
            self.completed = true;
            self.terminal.clone()
        }
    }
    impl Drop for Fake {
        fn drop(&mut self) {
            assert!(self.completed, "storage released before completion");
        }
    }
    fn fake(
        wait: Result<bool, String>,
        cancel: Result<(), String>,
        terminal: Result<u32, String>,
    ) -> Fake {
        Fake {
            buffer: vec![42].into_boxed_slice(),
            wait,
            cancel,
            terminal,
            calls: Vec::new(),
            completed: false,
        }
    }

    #[test]
    fn pending_success_does_not_cancel() {
        let mut write = fake(Ok(true), Ok(()), Ok(64));
        assert_eq!(complete(&mut write).unwrap(), 64);
        assert_eq!(write.calls, ["wait", "terminal"]);
    }
    #[test]
    fn timeout_cancels_and_retains_storage_through_terminal_abort() {
        let mut write = fake(Ok(false), Ok(()), Err("operation aborted".into()));
        assert!(complete(&mut write)
            .unwrap_err()
            .contains("timed out after 5000 ms"));
        assert_eq!(write.calls, ["wait", "cancel", "terminal"]);
        assert_eq!(write.buffer[0], 7);
    }
    #[test]
    fn cancellation_race_still_drains_and_reports_timeout() {
        let mut write = fake(Ok(false), Err("not found".into()), Ok(64));
        assert!(complete(&mut write).is_err());
        assert_eq!(write.calls, ["wait", "cancel", "terminal"]);
    }
    #[test]
    fn wait_failure_cancels_and_drains_even_when_cancellation_fails() {
        let mut write = fake(
            Err("wait failed".into()),
            Err("cancel failed".into()),
            Err("device gone".into()),
        );
        assert!(complete(&mut write).unwrap_err().contains("wait failed"));
        assert_eq!(write.calls, ["wait", "cancel", "terminal"]);
    }
}
