//! One lightweight reminder on each repository-managed inference request.
//! This common caller has no verified OP-DBUS principal. The wording therefore
//! directs discovery and does not claim the caller can read a graph.
use crate::provider::ChatMessage;

pub const SYSTEM_GRAPH_TURN_REMINDER: &str = "System graph: discover cozo.list_system_graphs through identity-bound 3tchedFS; check provenance and coverage; grants apply.";

/// Work on a request copy, preserving caller messages, tool results and order.
/// A leading reminder inserted by another repository-owned boundary is reused
/// so manager-backed tool loops do not duplicate it.
pub fn with_system_graph_reminder(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    if let Some(system) = messages.iter_mut().find(|message| message.role == "system") {
        if system.content != SYSTEM_GRAPH_TURN_REMINDER
            && !system
                .content
                .starts_with(&format!("{SYSTEM_GRAPH_TURN_REMINDER}\n"))
        {
            system.content = format!("{SYSTEM_GRAPH_TURN_REMINDER}\n{}", system.content);
        }
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
}
