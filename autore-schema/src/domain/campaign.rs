//! Campaign entity — a coordinated set of analysis tasks.
//!
//! A `Campaign` groups tasks, claims, and evidence produced during
//! a directed analysis effort. Its state machine governs the lifecycle:
//! `Pending → Active → Paused/Complete | Active → Complete → Archived`.

use autore_core::{Error, Result};
use crate::ids::CampaignId;

/// The lifecycle state of an analysis campaign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CampaignState {
    /// Campaign has been created but not yet started.
    Pending,
    /// Campaign is actively being executed.
    Active,
    /// Campaign execution has been temporarily suspended.
    Paused,
    /// All campaign tasks have been completed.
    Complete,
    /// Campaign cannot proceed due to unresolved dependencies or errors.
    Blocked,
}

impl CampaignState {
    /// Returns `true` if this state allows starting new tasks.
    pub fn can_start_tasks(&self) -> bool {
        matches!(self, CampaignState::Active)
    }

    /// Returns `true` if this state represents a terminal condition.
    pub fn is_terminal(&self) -> bool {
        matches!(self, CampaignState::Complete)
    }
}

/// An analysis campaign — a coordinated set of tasks to analyze a binary.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Campaign {
    /// Unique identifier for this campaign.
    pub id: CampaignId,
    /// Human-readable name for this campaign.
    pub name: String,
    /// Current lifecycle state.
    pub state: CampaignState,
}

impl Campaign {
    /// Creates a new campaign in the `Pending` state.
    pub fn new(id: CampaignId, name: impl Into<String>) -> Self {
        Campaign {
            id,
            name: name.into(),
            state: CampaignState::Pending,
        }
    }

    /// Transitions from `Pending` to `Active`.
    pub fn start(&mut self) -> Result<()> {
        match self.state {
            CampaignState::Pending => {
                self.state = CampaignState::Active;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot start campaign in state {:?}",
                self.state
            ))),
        }
    }

    /// Transitions from `Active` to `Paused`.
    pub fn pause(&mut self) -> Result<()> {
        match self.state {
            CampaignState::Active => {
                self.state = CampaignState::Paused;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot pause campaign in state {:?}",
                self.state
            ))),
        }
    }

    /// Transitions from `Paused` back to `Active`.
    pub fn resume(&mut self) -> Result<()> {
        match self.state {
            CampaignState::Paused => {
                self.state = CampaignState::Active;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot resume campaign in state {:?}",
                self.state
            ))),
        }
    }

    /// Marks the campaign as complete. Accepts any non-terminal state.
    pub fn complete(&mut self) -> Result<()> {
        if self.state.is_terminal() {
            return Err(Error::Validation(format!(
                "cannot complete campaign already in state {:?}",
                self.state
            )));
        }
        self.state = CampaignState::Complete;
        Ok(())
    }

    /// Blocks the campaign to indicate it cannot proceed.
    pub fn block(&mut self) -> Result<()> {
        match self.state {
            CampaignState::Active | CampaignState::Pending | CampaignState::Paused => {
                self.state = CampaignState::Blocked;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot block campaign in state {:?}",
                self.state
            ))),
        }
    }

    /// Unblocks a blocked campaign, returning it to `Active`.
    pub fn unblock(&mut self) -> Result<()> {
        match self.state {
            CampaignState::Blocked => {
                self.state = CampaignState::Active;
                Ok(())
            }
            _ => Err(Error::Validation(format!(
                "cannot unblock campaign in state {:?}",
                self.state
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_campaign() -> Campaign {
        Campaign::new(CampaignId::new(), "test-campaign")
    }

    #[test]
    fn campaign_starts_pending() {
        let c = sample_campaign();
        assert_eq!(c.state, CampaignState::Pending);
        assert!(!c.state.can_start_tasks());
        assert!(!c.state.is_terminal());
    }

    #[test]
    fn campaign_start_transitions_to_active() {
        let mut c = sample_campaign();
        c.start().unwrap();
        assert_eq!(c.state, CampaignState::Active);
        assert!(c.state.can_start_tasks());
    }

    #[test]
    fn campaign_pause_and_resume() {
        let mut c = sample_campaign();
        c.start().unwrap();
        c.pause().unwrap();
        assert_eq!(c.state, CampaignState::Paused);
        c.resume().unwrap();
        assert_eq!(c.state, CampaignState::Active);
    }

    #[test]
    fn campaign_complete_from_active() {
        let mut c = sample_campaign();
        c.start().unwrap();
        c.complete().unwrap();
        assert_eq!(c.state, CampaignState::Complete);
        assert!(c.state.is_terminal());
    }

    #[test]
    fn campaign_rejects_invalid_transitions() {
        let mut c = sample_campaign();
        // Cannot pause from Pending
        assert!(c.pause().is_err());
        // Cannot resume from Pending
        assert!(c.resume().is_err());
        // Start properly
        c.start().unwrap();
        // Cannot start twice
        assert!(c.start().is_err());
        // Cannot complete twice
        c.complete().unwrap();
        assert!(c.complete().is_err());
    }

    #[test]
    fn campaign_block_and_unblock() {
        let mut c = sample_campaign();
        c.start().unwrap();
        c.block().unwrap();
        assert_eq!(c.state, CampaignState::Blocked);
        c.unblock().unwrap();
        assert_eq!(c.state, CampaignState::Active);
        // Cannot block from complete
        c.complete().unwrap();
        assert!(c.block().is_err());
    }

    #[test]
    fn campaign_serialize_roundtrip() {
        let mut c = sample_campaign();
        c.start().unwrap();
        let json = serde_json::to_string(&c).unwrap();
        let deserialized: Campaign = serde_json::from_str(&json).unwrap();
        assert_eq!(c.id, deserialized.id);
        assert_eq!(c.name, deserialized.name);
        assert_eq!(deserialized.state, CampaignState::Active);
    }
}
