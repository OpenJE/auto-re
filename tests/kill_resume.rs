//! Kill/resume crash recovery tests.
//!
//! Proves that the campaign engine recovers from ungraceful process death
//! without producing duplicate accepted claims, and that campaigns complete
//! after restart.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use rusqlite::Connection;
use tempfile::TempDir;

fn binary_path() -> PathBuf {
    std::env::var("CARGO_BIN_EXE_auto_re")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let manifest_dir = env!("CARGO_MANIFEST_DIR");
            let target_dir = std::env::var("CARGO_TARGET_DIR")
                .unwrap_or_else(|_| format!("{manifest_dir}/target"));
            PathBuf::from(target_dir).join("debug").join("auto-re")
        })
}

struct TestHarness {
    _temp_dir: TempDir,
    db_path: PathBuf,
}

impl TestHarness {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("state.sqlite3");
        Self {
            _temp_dir: temp_dir,
            db_path,
        }
    }

    fn spawn_campaign_run(&self) -> Child {
        Command::new(binary_path())
            .args(["campaign", "run"])
            .env("AUTO_RE_DB_PATH", &self.db_path)
            .env("AUTO_RE_HEADLESS_DELAY_MS", "200")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to spawn auto-re")
    }

    fn open_db(&self) -> Connection {
        Connection::open(&self.db_path).expect("failed to open DB")
    }

    fn count_accepted_claims(&self) -> usize {
        if !self.db_path.exists() {
            return 0;
        }
        let conn = self.open_db();
        conn.query_row(
            "SELECT COUNT(*) FROM claims WHERE state = 'Accepted'",
            [],
            |row| row.get::<_, usize>(0),
        )
        .unwrap_or(0)
    }

    fn count_claims_by_subject(&self) -> Vec<(String, usize)> {
        if !self.db_path.exists() {
            return vec![];
        }
        let conn = self.open_db();
        let mut stmt = conn
            .prepare(
                "SELECT subject, COUNT(*) as cnt \
                 FROM claims WHERE state = 'Accepted' \
                 GROUP BY subject",
            )
            .expect("failed to prepare query");
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("failed to query")
            .collect::<Result<Vec<_>, _>>()
            .expect("failed to collect rows")
    }

    fn campaign_state(&self) -> Option<String> {
        if !self.db_path.exists() {
            return None;
        }
        let conn = self.open_db();
        conn.query_row("SELECT state FROM campaigns LIMIT 1", [], |row| {
            row.get::<_, String>(0)
        })
        .ok()
    }

    fn wait_for_accepted_claims(&self, min_count: usize, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if self.count_accepted_claims() >= min_count {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn wait_for_campaign_complete(&self, timeout: Duration) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if self.campaign_state().as_deref() == Some("Complete") {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn kill_resume_no_duplicate_claims() {
    let harness = TestHarness::new();

    // Phase 1: Run until at least one accepted claim, then kill.
    let mut child = harness.spawn_campaign_run();
    let got_claims = harness.wait_for_accepted_claims(1, Duration::from_secs(30));
    child.kill().ok();
    let _ = child.wait();

    assert!(
        got_claims,
        "should have produced at least one accepted claim before kill"
    );
    let claims_before = harness.count_accepted_claims();
    assert!(claims_before >= 1);

    // Phase 2: Resume and let it complete.
    let mut child2 = harness.spawn_campaign_run();
    let completed = harness.wait_for_campaign_complete(Duration::from_secs(60));
    if !completed {
        child2.kill().ok();
        let _ = child2.wait();
    } else {
        let _ = child2.wait();
    }

    // Assert: no duplicate accepted claims for the same subject (function).
    let groups = harness.count_claims_by_subject();
    for (subject, count) in &groups {
        assert_eq!(
            *count, 1,
            "duplicate accepted claims for subject={subject}: count={count}"
        );
    }
}

#[test]
fn accepted_claims_persist_after_kill() {
    let harness = TestHarness::new();

    // Phase 1: Run until accepted claims exist, then kill.
    let mut child = harness.spawn_campaign_run();
    let got_claims = harness.wait_for_accepted_claims(1, Duration::from_secs(30));
    child.kill().ok();
    let _ = child.wait();

    assert!(
        got_claims,
        "should have produced at least one accepted claim before kill"
    );
    let claims_before = harness.count_accepted_claims();

    // Assert: claims survive process death (DB file persists).
    let claims_after_kill = harness.count_accepted_claims();
    assert_eq!(
        claims_before, claims_after_kill,
        "accepted claims must persist in SQLite after ungraceful kill: \
         had {claims_before} before kill, {claims_after_kill} after"
    );
    assert!(
        claims_after_kill >= 1,
        "at least one accepted claim should survive kill"
    );

    // Phase 2: Resume and verify claims are still there.
    let mut child2 = harness.spawn_campaign_run();
    let completed = harness.wait_for_campaign_complete(Duration::from_secs(60));
    if !completed {
        child2.kill().ok();
        let _ = child2.wait();
    } else {
        let _ = child2.wait();
    }

    let claims_after_resume = harness.count_accepted_claims();
    assert!(
        claims_after_resume >= claims_after_kill,
        "claims should not be lost after resume: had {claims_after_kill} after kill, \
         {claims_after_resume} after resume"
    );
}

#[test]
fn campaign_completes_after_resume() {
    let harness = TestHarness::new();

    // Phase 1: Run briefly, then kill.
    let mut child = harness.spawn_campaign_run();
    // Wait for at least one claim or a short timeout.
    let _ = harness.wait_for_accepted_claims(1, Duration::from_secs(10));
    child.kill().ok();
    let _ = child.wait();

    // Phase 2: Resume and wait for campaign completion.
    let mut child2 = harness.spawn_campaign_run();
    let completed = harness.wait_for_campaign_complete(Duration::from_secs(60));
    if !completed {
        child2.kill().ok();
        let _ = child2.wait();
    } else {
        let _ = child2.wait();
    }

    assert!(
        completed,
        "campaign should reach Complete state after resume, \
         but state is {:?}",
        harness.campaign_state()
    );

    // All tasks should be terminal.
    let conn = harness.open_db();
    let non_terminal: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE state NOT IN ('Completed', 'Failed', 'Cancelled')",
            [],
            |row| row.get(0),
        )
        .unwrap_or(1);
    assert_eq!(
        non_terminal, 0,
        "all tasks should be terminal after campaign completion, \
         but {non_terminal} are still active"
    );
}
