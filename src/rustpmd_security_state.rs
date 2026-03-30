use super::*;
use crate::runtime_policy::WebSecurityPolicy;

pub(super) fn prune_sessions(
    sessions: &mut HashMap<String, SessionState>,
    now: u64,
    policy: &WebSecurityPolicy,
) {
    sessions.retain(|_, session| session.expires_at > now);
    if sessions.len() <= policy.max_web_sessions {
        return;
    }
    let mut keys: Vec<(String, u64)> = sessions
        .iter()
        .map(|(k, v)| (k.clone(), v.expires_at))
        .collect();
    keys.sort_by_key(|(_, expires_at)| *expires_at);
    let overflow = sessions.len().saturating_sub(policy.max_web_sessions);
    for (key, _) in keys.into_iter().take(overflow) {
        sessions.remove(&key);
    }
}

pub(super) fn prune_login_failures(
    failures: &mut HashMap<String, LoginFailureState>,
    now: u64,
    policy: &WebSecurityPolicy,
) {
    failures.retain(|_, state| {
        if state.blocked_until > now {
            return true;
        }
        now.saturating_sub(state.last_updated_at) <= policy.login_failure_ttl_secs
    });
    if failures.len() <= policy.max_login_failure_keys {
        return;
    }
    let mut keys: Vec<(String, u64)> = failures
        .iter()
        .map(|(k, v)| (k.clone(), v.last_updated_at))
        .collect();
    keys.sort_by_key(|(_, updated_at)| *updated_at);
    let overflow = failures.len().saturating_sub(policy.max_login_failure_keys);
    for (key, _) in keys.into_iter().take(overflow) {
        failures.remove(&key);
    }
}

pub(super) fn mark_login_failure(
    failures: &mut HashMap<String, LoginFailureState>,
    key: &str,
    now: u64,
    policy: &WebSecurityPolicy,
) {
    let entry = failures.entry(key.to_string()).or_default();
    entry.last_updated_at = now;
    entry.count = entry.count.saturating_add(1);
    if entry.count >= policy.login_block_threshold {
        entry.blocked_until = now + policy.login_block_secs;
        entry.count = 0;
    }
}

pub(super) fn clear_login_failure(failures: &mut HashMap<String, LoginFailureState>, key: &str) {
    failures.remove(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_sessions_enforces_capacity() {
        let now = 1_700_000_000_u64;
        let policy = WebSecurityPolicy::default();
        let mut sessions = HashMap::new();
        for i in 0..(policy.max_web_sessions + 64) {
            sessions.insert(
                format!("s-{}", i),
                SessionState {
                    expires_at: now + (i as u64),
                    user_agent_hash: "ua".to_string(),
                    ip_prefix: None,
                },
            );
        }
        prune_sessions(&mut sessions, now, &policy);
        assert!(sessions.len() <= policy.max_web_sessions);
    }

    #[test]
    fn prune_login_failures_removes_stale_entries() {
        let now = 1_700_000_000_u64;
        let policy = WebSecurityPolicy::default();
        let mut failures = HashMap::new();
        failures.insert(
            "stale".to_string(),
            LoginFailureState {
                count: 1,
                blocked_until: 0,
                last_updated_at: now - policy.login_failure_ttl_secs - 10,
            },
        );
        failures.insert(
            "fresh".to_string(),
            LoginFailureState {
                count: 1,
                blocked_until: 0,
                last_updated_at: now,
            },
        );
        prune_login_failures(&mut failures, now, &policy);
        assert!(!failures.contains_key("stale"));
        assert!(failures.contains_key("fresh"));
    }
}
