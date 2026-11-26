//! ACP advertises slash commands per live session; an update replaces its previous list.
use super::{ChatSession, KarBridgeEvent};
use crate::{studio_hub::SlashCommandItem, types::runtime_display_name};
use kar::{Direction, Protocol, RecordKind};
use serde_json::Value;
use tokio::sync::watch;

pub(super) fn watch(
    session: &ChatSession,
    sender: async_channel::Sender<KarBridgeEvent>,
    mut cancel: watch::Receiver<bool>,
) {
    let info = session.info();
    if info.protocol != Protocol::Acp {
        return;
    }
    let Ok(mut records) = session.subscribe(None) else {
        return;
    };
    let chat_id = info.id;
    let native_id = info.native_id.clone();
    let source = runtime_display_name(&info.runtime);
    tokio::spawn(async move {
        let mut previous = None;
        loop {
            let record = tokio::select! {
                biased;
                _ = cancel.changed() => return,
                record = records.next() => match record {
                    Some(record) => record,
                    None => return,
                },
            };
            if record.direction != Direction::FromRuntime
                || record.kind != RecordKind::Frame
                || !record.agent_path.is_empty()
            {
                continue;
            }
            if let Some(commands) = available_commands(&record.native, &native_id, &source)
                && previous.as_ref() != Some(&commands)
            {
                previous = Some(commands.clone());
                if sender
                    .send(KarBridgeEvent::SessionCommandsUpdated { chat_id, commands })
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    });
}

fn available_commands(
    packet: &Value,
    native_id: &str,
    source: &str,
) -> Option<Vec<SlashCommandItem>> {
    if packet.get("method")?.as_str()? != "session/update" || packet.get("id").is_some() {
        return None;
    }
    let params = packet.get("params")?;
    let update = params.get("update")?;
    if params.get("sessionId")?.as_str()? != native_id
        || update.get("sessionUpdate")?.as_str()? != "available_commands_update"
    {
        return None;
    }

    let items = update.get("availableCommands")?.as_array()?;
    let mut commands: Vec<SlashCommandItem> = Vec::with_capacity(items.len());
    for item in items {
        let Some(name) = item
            .get("name")
            .and_then(Value::as_str)
            .and_then(SlashCommandItem::normalized_name)
        else {
            continue;
        };
        if commands.iter().any(|known| known.name == name) {
            continue;
        }
        commands.push(SlashCommandItem {
            name,
            description: item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            hint: item
                .pointer("/input/hint")
                .and_then(Value::as_str)
                .map(str::to_owned),
            source: source.to_owned(),
        });
    }
    (items.is_empty() || !commands.is_empty()).then_some(commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn acp_commands_are_scoped_to_the_native_session_and_replaceable_with_an_empty_list() {
        let mut packet = json!({
            "method":"session/update",
            "params":{"sessionId":"thread-1", "update":{
                "sessionUpdate":"available_commands_update",
                "availableCommands":[
                    {"name":"review", "description":"Review changes", "input":{"hint":"<path>"}},
                    {"name":"/review", "description":"Duplicate"},
                    {"name":"two words"},
                    {"name":"ext:status", "description":"Show status"}
                ]
            }}
        });
        let commands = available_commands(&packet, "thread-1", "Agent").unwrap();
        assert_eq!(commands.len(), 2);
        assert_eq!(
            commands[0],
            SlashCommandItem {
                name: "/review".into(),
                description: "Review changes".into(),
                hint: Some("<path>".into()),
                source: "Agent".into()
            }
        );
        assert_eq!(commands[1].name, "/ext:status");
        assert!(available_commands(&packet, "another-thread", "Agent").is_none());
        packet["params"]["update"]["availableCommands"] = json!([]);
        assert_eq!(
            available_commands(&packet, "thread-1", "Agent"),
            Some(Vec::new())
        );
        packet["params"]["update"]["availableCommands"] = Value::Null;
        assert!(available_commands(&packet, "thread-1", "Agent").is_none());
    }
}
