//! Retain one raw tool result only while its Output inspector is selected.
use super::*;

#[wasm_bindgen(
    inline_js = "export function reportTrajectoryResultError(message) { console.warn('工具原始结果订阅不可用：' + message); }"
)]
extern "C" {
    #[wasm_bindgen(js_name = reportTrajectoryResultError)]
    fn report_error(message: &str);
}

pub(super) struct ResultWatch {
    session: String,
    seq: u64,
    token: String,
    update: u64,
    value: Option<Value>,
}

impl DshApp {
    pub(crate) fn sync_trajectory_result_watch(&mut self, cx: &mut Context<Self>) {
        let connected = self.connected();
        let reconnected = connected && !self.trajectory.result_watch_connected;
        self.trajectory.result_watch_connected = connected;
        let target = if self.connected()
            && self.trajectory.active == ConversationView::Trajectory
            && self.trajectory.detail_tab == DetailTab::Output
        {
            self.active_session
                .as_ref()
                .zip(self.trajectory.selected.as_ref())
                .and_then(|(session, key)| {
                    self.conversation
                        .trajectory
                        .records
                        .iter()
                        .find(|record| &record.key == key)
                        .and_then(|record| {
                            record.detail.result_seq.map(|seq| (session.clone(), seq))
                        })
                })
        } else {
            None
        };
        let previous = self
            .trajectory
            .result_watch
            .as_ref()
            .map(|watch| (watch.session.clone(), watch.seq));
        if target == previous && !reconnected {
            return;
        }
        let token = format!(
            "trajectory-{}-{}",
            js_sys::Date::now(),
            js_sys::Math::random().to_bits()
        );
        self.trajectory.result_watch = target.as_ref().map(|(session, seq)| ResultWatch {
            session: session.clone(),
            seq: *seq,
            token: token.clone(),
            update: 0,
            value: None,
        });
        if !self.connected() {
            return;
        }
        let (session, seq) = target.unzip();
        let args = json!({"sessionId": session, "seq": seq, "token": token});
        cx.spawn(async move |_, _| {
            // The Host publishes the initial value and changes through the same
            // ordered channel; a late request acknowledgement is never a value.
            if let Err(error) = bridge::invoke("watch_tool_result", args).await {
                report_error(&error);
            }
        })
        .detach();
    }

    pub(crate) fn accept_trajectory_tool_result(&mut self, event: &Value, cx: &mut Context<Self>) {
        let Some(watch) = self.trajectory.result_watch.as_mut() else {
            return;
        };
        if event["token"].as_str() != Some(&watch.token)
            || event["sessionId"].as_str() != Some(&watch.session)
            || event["seq"].as_u64() != Some(watch.seq)
        {
            return;
        }
        let Some(update) = event["update"]
            .as_u64()
            .filter(|update| *update > watch.update)
        else {
            return;
        };
        watch.update = update;
        watch.value = (event["available"] == true)
            .then(|| event.get("value").cloned())
            .flatten();
        cx.notify();
    }

    pub(super) fn trajectory_raw_result(&self, record: &Record) -> Option<&Value> {
        self.trajectory
            .result_watch
            .as_ref()
            .filter(|watch| {
                Some(&watch.session) == self.active_session.as_ref()
                    && Some(watch.seq) == record.detail.result_seq
                    && Some(&record.key) == self.trajectory.selected.as_ref()
            })
            .and_then(|watch| watch.value.as_ref())
    }
}
