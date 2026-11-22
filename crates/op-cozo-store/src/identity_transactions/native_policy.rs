//! The only permitted configuration change during an explicit root upgrade.
use super::*;

pub const IDENTITY_STORAGE_SYSCALL_DENY: &str = "ioctl errno 1 [1,0x4008941a,SCMP_CMP_MASKED_EQ,0xffffffff]\nioctl errno 1 [1,0x5000940f,SCMP_CMP_MASKED_EQ,0xffffffff]\nioctl errno 1 [1,0x5000943f,SCMP_CMP_MASKED_EQ,0xffffffff]";

pub fn native_root_policy_transition_matches(before: &str, after: &str) -> bool {
    let (Ok(mut before), Ok(after)) = (serde_json::from_str::<Value>(before), serde_json::from_str::<Value>(after))
        else { return false };
    for field in ["config", "expanded_config"] {
        let Some(config) = before[field].as_object_mut() else { return false };
        if config.get("security.syscalls.deny").and_then(Value::as_str)
            .is_some_and(|deny| !deny.is_empty() && deny != IDENTITY_STORAGE_SYSCALL_DENY)
            || after[field]["security.syscalls.deny_default"] != "true"
            || after[field]["security.syscalls.deny"] != IDENTITY_STORAGE_SYSCALL_DENY
        { return false; }
        config.insert("security.syscalls.deny_default".into(), "true".into());
        config.insert("security.syscalls.deny".into(), IDENTITY_STORAGE_SYSCALL_DENY.into());
    }
    let (Ok(before), Ok(after)) = (serde_json::to_string(&before), serde_json::to_string(&after))
        else { return false };
    clean_provision_instance_binding_matches(&before, &after)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_policy_allows_only_two_exact_safety_keys() {
        let before = serde_json::json!({"name":"Jeremy","type":"container","status":"Stopped","status_code":102,
            "config":{"volatile.uuid":"cc84a2d6-acc8-4327-b80b-9c243d6eda6e","security.idmap.isolated":"true"},
            "expanded_config":{"volatile.uuid":"cc84a2d6-acc8-4327-b80b-9c243d6eda6e","security.idmap.isolated":"true"},
            "devices":[],"expanded_devices":[]});
        let mut after = before.clone();
        for field in ["config", "expanded_config"] {
            after[field]["security.syscalls.deny_default"] = "true".into();
            after[field]["security.syscalls.deny"] = IDENTITY_STORAGE_SYSCALL_DENY.into();
        }
        assert!(native_root_policy_transition_matches(&before.to_string(), &after.to_string()));
        for (key, value) in [("security.privileged", "true"), ("raw.lxc", "lxc.mount.entry = / /host none bind 0 0"),
            ("volatile.idmap.base", "0"), ("security.protection.shift", "false")] {
            let mut drift = after.clone(); drift["config"][key] = value.into();
            assert!(!native_root_policy_transition_matches(&before.to_string(), &drift.to_string()), "{key}");
        }
        let mut drift = after.clone(); drift["devices"] = serde_json::json!([{"name":"host","device":{"type":"disk","source":"/","path":"/host"}}]);
        assert!(!native_root_policy_transition_matches(&before.to_string(), &drift.to_string()));
        assert!(!native_root_policy_transition_matches(&before.to_string(), &before.to_string()));
        let mut old_deny = before;
        old_deny["config"]["security.syscalls.deny"] = "unrelated existing restriction".into();
        assert!(!native_root_policy_transition_matches(&old_deny.to_string(), &after.to_string()));
    }
}
