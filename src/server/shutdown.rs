//! Why the server is stopping, recorded where the request arrives and logged
//! once when shutdown begins.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::platform::ServerQuitSignal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShutdownReason {
    Signal(ServerQuitSignal),
    ApiStop { caller: Option<String> },
    HostShutdown,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiStopRequest {
    Accepted,
    AlreadyPending,
    WaiterUnavailable,
}

impl std::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signal(signal) => write!(f, "{signal}"),
            Self::ApiStop {
                caller: Some(caller),
            } => write!(f, "server.stop request from {caller}"),
            Self::ApiStop { caller: None } => f.write_str("server.stop request"),
            Self::HostShutdown => f.write_str("host shutdown"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

/// The server's stop flag plus the reason recorded by the first stop request.
#[derive(Clone, Default)]
pub(crate) struct ServerStop {
    flag: Arc<AtomicBool>,
    reason: Arc<Mutex<Option<ShutdownReason>>>,
    api_stop_pending: Arc<AtomicBool>,
}

impl ServerStop {
    pub(crate) fn flag(&self) -> &Arc<AtomicBool> {
        &self.flag
    }

    pub(crate) fn is_requested(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }

    pub(crate) fn is_unavailable(&self) -> bool {
        self.is_requested() || self.api_stop_pending.load(Ordering::Acquire)
    }

    pub(crate) fn request(&self, reason: ShutdownReason) {
        let Ok(mut recorded) = self.reason.lock() else {
            self.flag.store(true, Ordering::Release);
            return;
        };
        if !self.flag.load(Ordering::Acquire) {
            *recorded = Some(reason);
            self.flag.store(true, Ordering::Release);
        }
    }

    pub(crate) fn request_api_after_response(
        &self,
        caller: Option<String>,
        response_write_complete: Option<std::sync::mpsc::Receiver<()>>,
        grace: Duration,
    ) -> ApiStopRequest {
        self.request_api_with_spawner(caller, response_write_complete, grace, |task| {
            std::thread::Builder::new()
                .name("herdr-stop-ack-wait".into())
                .spawn(task)
                .map(|_| ())
        })
    }

    fn request_api_with_spawner(
        &self,
        caller: Option<String>,
        response_write_complete: Option<std::sync::mpsc::Receiver<()>>,
        grace: Duration,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> std::io::Result<()>,
    ) -> ApiStopRequest {
        if self.is_requested()
            || self
                .api_stop_pending
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return ApiStopRequest::AlreadyPending;
        }
        let reason = ShutdownReason::ApiStop { caller };
        if let Some(response_write_complete) = response_write_complete {
            let stop = self.clone();
            let deferred_reason = reason.clone();
            if spawn(Box::new(move || {
                let _ = response_write_complete.recv_timeout(grace);
                stop.request(deferred_reason);
            }))
            .is_ok()
            {
                return ApiStopRequest::Accepted;
            }
            self.request(reason);
            return ApiStopRequest::WaiterUnavailable;
        }
        self.request(reason);
        ApiStopRequest::Accepted
    }

    pub(crate) fn take_reason(&self) -> Option<ShutdownReason> {
        self.reason.lock().ok()?.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_stop(stop: &ServerStop) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !stop.is_requested() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(stop.is_requested(), "stop was not requested");
    }

    #[test]
    fn api_stop_waits_for_write_completion_and_rejects_concurrent_requests() {
        let stop = ServerStop::default();
        let (tx, rx) = std::sync::mpsc::channel();
        assert_eq!(
            stop.request_api_after_response(Some("first".into()), Some(rx), Duration::from_secs(3)),
            ApiStopRequest::Accepted
        );
        assert!(!stop.is_requested());
        assert!(stop.is_unavailable());
        let (other_tx, other_rx) = std::sync::mpsc::channel();
        assert_eq!(
            stop.request_api_after_response(
                Some("second".into()),
                Some(other_rx),
                Duration::from_secs(3)
            ),
            ApiStopRequest::AlreadyPending
        );
        assert!(!stop.is_requested());
        tx.send(()).unwrap();
        wait_for_stop(&stop);
        assert_eq!(
            stop.take_reason(),
            Some(ShutdownReason::ApiStop {
                caller: Some("first".into())
            })
        );
        drop(other_tx);
    }

    #[test]
    fn api_stop_falls_back_when_write_does_not_complete() {
        let stop = ServerStop::default();
        let (tx, rx) = std::sync::mpsc::channel();
        assert_eq!(
            stop.request_api_after_response(None, Some(rx), Duration::from_millis(30)),
            ApiStopRequest::Accepted
        );
        wait_for_stop(&stop);
        assert_eq!(
            stop.take_reason(),
            Some(ShutdownReason::ApiStop { caller: None })
        );
        drop(tx);
    }

    #[test]
    fn api_stop_falls_back_when_writer_disappears() {
        let stop = ServerStop::default();
        let (tx, rx) = std::sync::mpsc::channel();
        assert_eq!(
            stop.request_api_after_response(None, Some(rx), Duration::from_secs(3)),
            ApiStopRequest::Accepted
        );
        drop(tx);
        wait_for_stop(&stop);
    }

    #[test]
    fn concurrent_stop_requests_schedule_only_one_waiter() {
        let stop = ServerStop::default();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let stop = stop.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                let (tx, rx) = std::sync::mpsc::channel();
                barrier.wait();
                let accepted =
                    stop.request_api_after_response(None, Some(rx), Duration::from_secs(3));
                (accepted, tx)
            }));
        }
        let requests: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(
            requests
                .iter()
                .filter(|(accepted, _)| *accepted == ApiStopRequest::Accepted)
                .count(),
            1
        );
        assert!(!stop.is_requested());
        for (accepted, tx) in requests {
            if accepted == ApiStopRequest::Accepted {
                tx.send(()).unwrap();
            }
        }
        wait_for_stop(&stop);
    }

    #[test]
    fn waiter_spawn_failure_reports_unknown_and_still_stops() {
        let stop = ServerStop::default();
        let (_tx, rx) = std::sync::mpsc::channel();
        let result = stop.request_api_with_spawner(None, Some(rx), Duration::from_secs(3), |_| {
            Err(std::io::Error::other("injected spawn failure"))
        });
        assert_eq!(result, ApiStopRequest::WaiterUnavailable);
        assert!(stop.is_unavailable());
        assert!(stop.is_requested());
        assert_eq!(
            stop.take_reason(),
            Some(ShutdownReason::ApiStop { caller: None })
        );
    }

    #[test]
    fn host_shutdown_during_ack_wait_is_immediate_and_first_reason_wins() {
        let stop = ServerStop::default();
        let (tx, rx) = std::sync::mpsc::channel();
        assert_eq!(
            stop.request_api_after_response(None, Some(rx), Duration::from_secs(3)),
            ApiStopRequest::Accepted
        );
        assert!(stop.is_unavailable());
        assert!(!stop.is_requested());
        stop.request(ShutdownReason::HostShutdown);
        assert!(stop.is_requested());
        assert_eq!(stop.take_reason(), Some(ShutdownReason::HostShutdown));
        tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while Arc::strong_count(&stop.flag) > 1 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            Arc::strong_count(&stop.flag),
            1,
            "deferred waiter did not finish"
        );
        assert_eq!(stop.take_reason(), None);
    }

    #[cfg(not(unix))]
    #[test]
    fn console_signal_during_ack_wait_is_not_delayed() {
        let stop = ServerStop::default();
        let (_tx, rx) = std::sync::mpsc::channel();
        assert_eq!(
            stop.request_api_after_response(None, Some(rx), Duration::from_secs(3)),
            ApiStopRequest::Accepted
        );
        stop.request(ShutdownReason::Signal(ServerQuitSignal::ConsoleControl));
        assert!(stop.is_requested());
        assert_eq!(
            stop.take_reason(),
            Some(ShutdownReason::Signal(ServerQuitSignal::ConsoleControl))
        );
    }

    #[test]
    fn first_stop_reason_wins() {
        let stop = ServerStop::default();
        assert!(!stop.is_requested());

        stop.request(ShutdownReason::HostShutdown);
        stop.request(ShutdownReason::ApiStop { caller: None });

        assert!(stop.is_requested());
        assert_eq!(stop.take_reason(), Some(ShutdownReason::HostShutdown));
        assert_eq!(stop.take_reason(), None);
    }

    #[test]
    fn api_stop_reason_names_the_caller() {
        let reason = ShutdownReason::ApiStop {
            caller: Some("pid 42 (herdr), parent pid 7 (zsh)".into()),
        };
        assert_eq!(
            reason.to_string(),
            "server.stop request from pid 42 (herdr), parent pid 7 (zsh)"
        );
    }
}
