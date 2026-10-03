//! Transactional endpoint-volume setup, shared with failure-injection tests.

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct State {
    pub scalar: f32,
    pub muted: bool,
}

pub(super) trait Endpoint {
    fn name(&self) -> &str;
    fn state(&self) -> Result<State, String>;
    fn set_scalar(&self, scalar: f32) -> Result<(), String>;
    fn set_muted(&self, muted: bool) -> Result<(), String>;
}

/// Contains only changed endpoints. Recording before either setter lets Drop
/// roll back a partial setup, including a volume write followed by a mute error.
pub(super) struct UnityHold<E: Endpoint> {
    prev: Vec<(E, State)>,
}

impl<E: Endpoint> UnityHold<E> {
    pub fn new(endpoints: impl IntoIterator<Item = E>) -> Result<Self, String> {
        let mut hold = Self { prev: Vec::new() };
        for endpoint in endpoints {
            let state = endpoint.state()?;
            if (state.scalar - 1.0).abs() > 0.001 || state.muted {
                hold.prev.push((endpoint, state));
                let endpoint = &hold.prev.last().expect("recorded endpoint").0;
                endpoint.set_scalar(1.0)?;
                endpoint.set_muted(false)?;
                log::warn!(
                    "[audio] {}: Windows endpoint volume was {:.0}%{} — holding it at 100% for this re-amp session; it is restored afterwards",
                    endpoint.name(), state.scalar * 100.0,
                    if state.muted { " and MUTED" } else { "" }
                );
            }
        }
        Ok(hold)
    }
}

impl<E: Endpoint> Drop for UnityHold<E> {
    fn drop(&mut self) {
        for (endpoint, state) in self.prev.iter().rev() {
            // Attempt both restorations even if one fails.
            if let Err(e) = endpoint.set_scalar(state.scalar) {
                log::warn!("[audio] failed to restore endpoint volume: {e}");
            }
            if let Err(e) = endpoint.set_muted(state.muted) {
                log::warn!("[audio] failed to restore endpoint mute: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone)]
    struct Fake {
        state: Rc<RefCell<State>>,
        fail_scalar: bool,
        fail_mute: bool,
    }
    impl Endpoint for Fake {
        fn name(&self) -> &str {
            "test TMP"
        }
        fn state(&self) -> Result<State, String> {
            Ok(*self.state.borrow())
        }
        fn set_scalar(&self, scalar: f32) -> Result<(), String> {
            if self.fail_scalar && scalar == 1.0 {
                return Err("volume failed".into());
            }
            self.state.borrow_mut().scalar = scalar;
            Ok(())
        }
        fn set_muted(&self, muted: bool) -> Result<(), String> {
            if self.fail_mute && !muted {
                return Err("mute failed".into());
            }
            self.state.borrow_mut().muted = muted;
            Ok(())
        }
    }
    fn fake(fail_scalar: bool, fail_mute: bool) -> Fake {
        Fake {
            state: Rc::new(RefCell::new(State {
                scalar: 0.08,
                muted: true,
            })),
            fail_scalar,
            fail_mute,
        }
    }

    #[test]
    fn partial_mute_failure_rolls_back_current_and_previous_endpoints() {
        let first = fake(false, false);
        let second = fake(false, true);
        let original = *first.state.borrow();
        assert!(UnityHold::new([first.clone(), second.clone()]).is_err());
        assert_eq!(*first.state.borrow(), original);
        assert_eq!(*second.state.borrow(), original);
    }

    #[test]
    fn volume_failure_rejects_setup_and_restores_prior_endpoints() {
        let first = fake(false, false);
        let second = fake(true, false);
        let original = *first.state.borrow();
        assert!(UnityHold::new([first.clone(), second.clone()]).is_err());
        assert_eq!(*first.state.borrow(), original);
        assert_eq!(*second.state.borrow(), original);
    }

    #[test]
    fn duplicate_names_restore_each_endpoints_distinct_state() {
        let first = fake(false, false);
        let second = fake(false, false);
        *second.state.borrow_mut() = State {
            scalar: 0.55,
            muted: false,
        };
        let originals = [*first.state.borrow(), *second.state.borrow()];
        assert_eq!(first.name(), second.name());
        let hold = UnityHold::new([first.clone(), second.clone()]).unwrap();
        for endpoint in [&first, &second] {
            assert_eq!(
                *endpoint.state.borrow(),
                State {
                    scalar: 1.0,
                    muted: false
                }
            );
        }
        drop(hold);
        assert_eq!(*first.state.borrow(), originals[0]);
        assert_eq!(*second.state.borrow(), originals[1]);
    }

    #[test]
    fn successful_hold_restores_only_after_session_resources_stop() {
        let endpoint = fake(false, false);
        let original = *endpoint.state.borrow();
        struct Streams(Fake);
        impl Drop for Streams {
            fn drop(&mut self) {
                assert_eq!(
                    *self.0.state.borrow(),
                    State {
                        scalar: 1.0,
                        muted: false
                    }
                );
            }
        }
        struct Session {
            _streams: Streams,
            _hold: UnityHold<Fake>,
        }
        let session = Session {
            _streams: Streams(endpoint.clone()),
            _hold: UnityHold::new([endpoint.clone()]).unwrap(),
        };
        drop(session);
        assert_eq!(*endpoint.state.borrow(), original);
    }
}
