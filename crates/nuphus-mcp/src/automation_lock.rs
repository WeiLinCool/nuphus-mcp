//! Cross-process automation lock.
//!
//! Desktop (mouse/keyboard/screen) and browser automation operate on **exclusive
//! machine resources** — at most one Agent may run an automation operation at any
//! given moment. Multiple nuphus-mcp instances (one per Agent via stdio transport)
//! coordinate through a shared lock file.
//!
//! Semantics (per the product decision):
//! - **Full coverage**: both `browser_*` and `desktop_*` tools acquire the lock.
//! - **Short hold**: the lock is held only for the duration of a single tool call;
//!   it is released immediately after the call (Drop guard), never bound to an Agent
//!   session. An idle server holds no lock, so any Agent can acquire it.
//! - **Busy rejection**: when another live holder exists, acquisition fails with a
//!   clear "busy" message instead of blocking; the caller should retry later.
//! - **Crash self-healing**: a stale lock from a crashed process expires via TTL
//!   (90s, comfortably above the longest tool timeout of 30s) and is reclaimed.
//!
//! Lock file location: `{data_dir}/Nuphus/nuphus-mcp/automation.lock` — deliberately
//! **outside** the browser profile directory, so it stays valid regardless of profile
//! layout and can later be shared with the main Nuphus app's direct channel.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// How long a stale lock may live before being reclaimed (crash self-healing).
/// Must stay above the longest tool timeout (30s navigate) + margin.
const LOCK_TTL_SECS: u64 = 90;

/// Lock file name (inside `{data_dir}/Nuphus/nuphus-mcp/`).
const LOCK_FILE_NAME: &str = "automation.lock";

/// Serialized holder info written into the lock file.
#[derive(Debug, Serialize, Deserialize)]
struct LockRecord {
    pid: u32,
    tool: String,
    acquired_at: u64,
    expires_at: u64,
}

/// Cross-process automation lock handle.
pub struct AutomationLock {
    path: PathBuf,
}

/// RAII guard: deleting the lock file releases the lock.
#[derive(Debug)]
pub struct LockGuard {
    path: PathBuf,
}

impl AutomationLock {
    /// Resolve the shared lock file path (cross-platform data dir).
    ///
    /// Under `#[cfg(test)]` the lock lives in the temp dir so unit/integration tests
    /// never touch the real user data dir (parallel tests would otherwise fight over
    /// the production lock file).
    pub fn new() -> Self {
        #[cfg(test)]
        {
            let dir = std::env::temp_dir().join("nuphus_mcp_lock_test");
            let _ = fs::create_dir_all(&dir);
            Self {
                path: dir.join(LOCK_FILE_NAME),
            }
        }
        #[cfg(not(test))]
        {
            let base =
                dirs::data_dir().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let dir = base.join("Nuphus").join("nuphus-mcp");
            let _ = fs::create_dir_all(&dir);
            Self {
                path: dir.join(LOCK_FILE_NAME),
            }
        }
    }

    /// Try to acquire the lock for a single tool call.
    ///
    /// Returns `Ok(guard)` when the lock is free (or a stale lock is reclaimed).
    /// Returns `Err(busy)` when another live holder owns it — the caller surfaces
    /// this as a semantic failure and the Agent retries later.
    pub fn acquire(&self, tool: &str) -> Result<LockGuard, String> {
        let now = unix_now();

        // Attempt an atomic create; fails with AlreadyExists when held.
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
        {
            Ok(mut file) => {
                use std::io::Write;
                let record = LockRecord {
                    pid: std::process::id(),
                    tool: tool.to_string(),
                    acquired_at: now,
                    expires_at: now + LOCK_TTL_SECS,
                };
                let payload = match serde_json::to_string(&record) {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = fs::remove_file(&self.path);
                        return Err(format!("automation lock serialize failed: {e}"));
                    }
                };
                if let Err(e) = file.write_all(payload.as_bytes()) {
                    let _ = fs::remove_file(&self.path);
                    return Err(format!("automation lock write failed: {e}"));
                }
                tracing::debug!(
                    "[automation-lock] acquired for '{tool}' (pid={})",
                    record.pid
                );
                Ok(LockGuard {
                    path: self.path.clone(),
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // Lock file exists: inspect the holder.
                match read_lock_record(&self.path) {
                    Some(rec) if rec.expires_at > now => {
                        // Live holder → busy. Report who holds it (tool + pid) so the
                        // Agent can decide when to retry.
                        Err(format!(
                            "Automation is busy: another Agent (pid={}) is running '{}'. \
                             Only one Agent can operate the desktop/browser at a time; retry later.",
                            rec.pid, rec.tool
                        ))
                    }
                    _ => {
                        // Stale (expired) or unreadable lock → reclaim it and retry once.
                        tracing::warn!(
                            "[automation-lock] stale lock detected, reclaiming ({})",
                            self.path.display()
                        );
                        let _ = fs::remove_file(&self.path);
                        self.acquire(tool)
                    }
                }
            }
            Err(e) => Err(format!("automation lock acquire failed: {e}")),
        }
    }

    /// Path of the lock file (exposed for tests / diagnostics).
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Only delete the lock if we still own it (pid matches). This prevents a
        // reclaiming process from deleting the new holder's lock on guard drop.
        let owns = read_lock_record(&self.path)
            .map(|r| r.pid == std::process::id())
            .unwrap_or(false);
        if owns {
            let _ = fs::remove_file(&self.path);
            tracing::debug!("[automation-lock] released (pid={})", std::process::id());
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn read_lock_record(path: &std::path::Path) -> Option<LockRecord> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Use a temp lock path to avoid polluting the real data dir during tests.
    fn temp_lock(dir_name: &str) -> AutomationLock {
        let path = std::env::temp_dir()
            .join("nuphus_mcp_lock_test")
            .join(dir_name);
        let _ = fs::create_dir_all(&path);
        AutomationLock {
            path: path.join(LOCK_FILE_NAME),
        }
    }

    #[test]
    fn acquire_release_cycle() {
        let lock = temp_lock("cycle");
        let _ = fs::remove_file(lock.path());
        {
            let guard = lock.acquire("browser_navigate").expect("first acquire ok");
            assert!(guard.path.exists());
        } // guard dropped → released
        assert!(!lock.path().exists(), "lock file removed after guard drop");
    }

    #[test]
    fn busy_while_held() {
        let lock = temp_lock("busy");
        let _ = fs::remove_file(lock.path());
        let _guard = lock.acquire("desktop_mouse").expect("first acquire ok");
        let err = lock
            .acquire("browser_click")
            .expect_err("second acquire busy");
        assert!(err.contains("busy"), "error mentions busy: {err}");
        assert!(
            err.contains("desktop_mouse"),
            "error names holder tool: {err}"
        );
    }

    #[test]
    fn stale_lock_is_reclaimed() {
        let lock = temp_lock("stale");
        let _ = fs::remove_file(lock.path());
        // Write an expired lock record (older than TTL) directly.
        let stale = LockRecord {
            pid: 999_999,
            tool: "browser_navigate".into(),
            acquired_at: 0,
            expires_at: unix_now().saturating_sub(1), // already expired
        };
        fs::write(lock.path(), serde_json::to_string(&stale).unwrap()).unwrap();
        let guard = lock
            .acquire("browser_snapshot")
            .expect("stale lock reclaimed");
        // New holder must be this process.
        let rec = read_lock_record(lock.path()).expect("record readable");
        assert_eq!(rec.pid, std::process::id());
        drop(guard);
    }

    #[test]
    fn guard_drop_does_not_delete_others_lock() {
        let lock = temp_lock("owner_check");
        let _ = fs::remove_file(lock.path());
        // Simulate: we hold the lock, then another process reclaims it (writes its
        // own record over ours — e.g. after our TTL expired). Dropping our guard
        // must NOT delete the new owner's lock.
        {
            let guard = lock.acquire("desktop_screenshot").expect("acquire ok");
            let stolen = LockRecord {
                pid: 888_888,
                tool: "desktop_mouse".into(),
                acquired_at: unix_now(),
                expires_at: unix_now() + LOCK_TTL_SECS,
            };
            fs::write(lock.path(), serde_json::to_string(&stolen).unwrap()).unwrap();
            drop(guard); // must not remove the file (pid differs)
            assert!(lock.path().exists(), "new owner's lock preserved");
        }
        let _ = fs::remove_file(lock.path());
    }
}
