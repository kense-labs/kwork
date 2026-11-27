//! Only the latest interactive sign-in can present a challenge in this window.
use uuid::Uuid;

#[derive(Default)]
pub(crate) struct ProviderSignIn {
    active: Option<Attempt>,
}

struct Attempt {
    id: Uuid,
    runtime: String,
    challenge: Option<Challenge>,
}

pub(crate) struct Challenge {
    pub url: String,
    pub device_code: Option<String>,
    pub message: String,
}

impl ProviderSignIn {
    pub(super) fn start(&mut self, id: Uuid, runtime: String) {
        self.active = Some(Attempt {
            id,
            runtime,
            challenge: None,
        });
    }

    pub(super) fn prompt(
        &mut self,
        id: Uuid,
        url: String,
        device_code: Option<String>,
        message: String,
    ) {
        if let Some(attempt) = self.active.as_mut().filter(|attempt| attempt.id == id) {
            attempt.challenge = Some(Challenge {
                url,
                device_code,
                message,
            });
        }
    }

    pub(crate) fn waiting_for(&self, runtime: &str) -> bool {
        self.active
            .as_ref()
            .is_some_and(|attempt| attempt.runtime == runtime)
    }

    pub(crate) fn challenge(&self) -> Option<(&str, &Challenge)> {
        let attempt = self.active.as_ref()?;
        Some((&attempt.runtime, attempt.challenge.as_ref()?))
    }

    pub(super) fn finish(&mut self, id: Uuid) {
        self.active.take_if(|attempt| attempt.id == id);
    }

    pub(super) fn signed_in(&mut self, runtime: &str) {
        self.active.take_if(|attempt| attempt.runtime == runtime);
    }

    pub(crate) fn dismiss(&mut self) {
        self.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send_challenge(state: &mut ProviderSignIn, id: Uuid) {
        state.prompt(
            id,
            "https://example.invalid/device".into(),
            Some("test-code".into()),
            "Verify".into(),
        );
    }

    #[test]
    fn confirmed_account_removes_its_challenge_and_ignores_late_messages() {
        let mut state = ProviderSignIn::default();
        let grok = Uuid::new_v4();
        state.start(grok, "grok".into());
        send_challenge(&mut state, grok);
        state.signed_in("codex");
        assert_eq!(state.challenge().unwrap().0, "grok");
        state.signed_in("grok");
        send_challenge(&mut state, grok);
        assert!(state.challenge().is_none());
        assert!(!state.waiting_for("grok"));
    }

    #[test]
    fn only_the_current_sign_in_can_update_or_finish_a_retry() {
        let mut state = ProviderSignIn::default();
        let first = Uuid::new_v4();
        let retry = Uuid::new_v4();
        state.start(first, "grok".into());
        state.dismiss();
        send_challenge(&mut state, first);
        assert!(state.challenge().is_none());
        state.start(retry, "grok".into());
        send_challenge(&mut state, first);
        assert!(state.challenge().is_none());
        send_challenge(&mut state, retry);
        state.finish(first);
        assert!(state.challenge().is_some());
        state.finish(retry);
        send_challenge(&mut state, retry);
        assert!(state.challenge().is_none());
    }
}
