//! One lightweight reminder on each repository-managed inference request.
//! This common caller has no verified OP-DBUS principal. The wording therefore
//! directs discovery and does not claim the caller can read a graph.
use crate::provider::ChatMessage;

pub const SYSTEM_GRAPH_TURN_REMINDER: &str = "System graph: discover cozo.list_system_graphs through identity-bound 3tchedFS; check provenance and coverage; grants apply.";

/// Work on a request copy, preserving caller messages, tool results and order.
/// Leading reminders inserted by repository-owned boundaries are consolidated
/// into the first system message. Other content, roles and message order remain.
pub fn with_system_graph_reminder(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let prefix = format!("{SYSTEM_GRAPH_TURN_REMINDER}\n");
    let mut first_system = None;
    for (index, message) in messages.iter_mut().enumerate() {
        if message.role != "system" {
            continue;
        }
        first_system.get_or_insert(index);
        let mut content = message.content.as_str();
        loop {
            if content == SYSTEM_GRAPH_TURN_REMINDER {
                content = "";
                break;
            }
            if let Some(rest) = content.strip_prefix(&prefix) {
                content = rest;
            } else {
                break;
            }
        }
        if content.len() != message.content.len() {
            message.content = content.to_owned();
        }
    }
    if let Some(index) = first_system {
        let system = &mut messages[index];
        system.content = if system.content.is_empty() {
            SYSTEM_GRAPH_TURN_REMINDER.to_owned()
        } else {
            format!("{SYSTEM_GRAPH_TURN_REMINDER}\n{}", system.content)
        };
    } else {
        messages.insert(
            0,
            ChatMessage::system(format!("{SYSTEM_GRAPH_TURN_REMINDER}\n")),
        );
    }
    messages
}

/// Providers with a separate system field must retain every trusted system
/// message in order. User and tool content never enters that field.
pub(crate) fn ordered_system_content(messages: &[ChatMessage]) -> Option<String> {
    let mut systems = messages.iter().filter(|message| message.role == "system");
    let mut content = systems.next()?.content.clone();
    for message in systems {
        content.push_str("\n\n");
        content.push_str(&message.content);
    }
    Some(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reminder_preserves_instruction_and_tool_conversation_in_each_request_copy() {
        let original = vec![
            ChatMessage::system("Existing instructions"),
            ChatMessage::user("Question"),
            ChatMessage::tool_result("call-id", "Result"),
        ];
        let first = with_system_graph_reminder(original.clone());
        let second = with_system_graph_reminder(original.clone());
        assert_eq!(
            first[0].content,
            format!("{SYSTEM_GRAPH_TURN_REMINDER}\nExisting instructions")
        );
        assert_eq!(first[1].content, "Question");
        assert_eq!(first[2].tool_call_id.as_deref(), Some("call-id"));
        assert_eq!(first[2].content, "Result");
        assert_eq!(second[0].content, first[0].content);
        assert_eq!(original[0].content, "Existing instructions");
        let nested = with_system_graph_reminder(first);
        assert_eq!(
            nested[0]
                .content
                .matches(SYSTEM_GRAPH_TURN_REMINDER)
                .count(),
            1
        );
        // The native factory emits a line without a trailing newline when
        // there are no other system instructions. Reuse that exact form too.
        let native =
            with_system_graph_reminder(vec![ChatMessage::system(SYSTEM_GRAPH_TURN_REMINDER)]);
        assert_eq!(native[0].content, SYSTEM_GRAPH_TURN_REMINDER);
    }
    #[test]
    fn user_text_cannot_suppress_trusted_reminder() {
        let request =
            with_system_graph_reminder(vec![ChatMessage::user(SYSTEM_GRAPH_TURN_REMINDER)]);
        assert_eq!(request.len(), 2);
        assert_eq!(request[0].role, "system");
        assert_eq!(request[0].content.lines().count(), 1);
        assert_eq!(request[1].content, SYSTEM_GRAPH_TURN_REMINDER);
    }

    #[test]
    fn multiple_system_messages_preserve_order_and_one_owned_reminder() {
        let original = vec![
            ChatMessage::system("First instructions"),
            ChatMessage::user("Question"),
            ChatMessage::system(format!("{SYSTEM_GRAPH_TURN_REMINDER}\nSecond instructions")),
            ChatMessage::tool_result("call-id", "Result"),
            ChatMessage::system(format!("{SYSTEM_GRAPH_TURN_REMINDER}\nThird instructions")),
        ];
        let request = with_system_graph_reminder(original.clone());
        assert_eq!(request.len(), original.len());
        for (before, after) in original.iter().zip(&request) {
            assert_eq!(before.role, after.role);
            assert_eq!(before.tool_call_id, after.tool_call_id);
        }
        assert_eq!(request[1].content, "Question");
        assert_eq!(request[3].content, "Result");
        let system = ordered_system_content(&request).unwrap();
        assert_eq!(system, format!(
            "{SYSTEM_GRAPH_TURN_REMINDER}\nFirst instructions\n\nSecond instructions\n\nThird instructions"
        ));
        assert_eq!(system.matches(SYSTEM_GRAPH_TURN_REMINDER).count(), 1);
        assert_eq!(
            ordered_system_content(&with_system_graph_reminder(request)),
            Some(system)
        );
        assert!(ordered_system_content(&[ChatMessage::user("untrusted")]).is_none());
    }
}
