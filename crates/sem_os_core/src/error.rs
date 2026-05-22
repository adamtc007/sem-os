use thiserror::Error;

// GateSeverity now lives in sem_os_types (sem_os_core-split v1 Phase 9).
pub(crate) use sem_os_types::GateSeverity;

#[derive(Debug, Error)]
pub enum SemOsError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("gate failed: {} violation(s)", .0.len())]
    GateFailed(Vec<GateViolation>),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("migration pending — {0}")]
    MigrationPending(String),

    #[error("internal: {0}")]
    Internal(#[from] anyhow::Error),
}

impl SemOsError {
    pub fn http_status(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::GateFailed(_) => 422,
            Self::Unauthorized(_) => 403,
            Self::Conflict(_) => 409,
            Self::InvalidInput(_) => 400,
            Self::MigrationPending(_) => 503,
            Self::Internal(_) => 500,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateViolation {
    pub gate_id: String,
    pub severity: GateSeverity,
    pub message: String,
    pub remediation: Option<String>,
}

impl std::fmt::Display for GateViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.gate_id, self.severity, self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── http_status: exhaustive variant coverage ──────────────────

    #[test]
    fn http_status_not_found() {
        assert_eq!(SemOsError::NotFound("x".into()).http_status(), 404);
    }

    #[test]
    fn http_status_gate_failed() {
        assert_eq!(SemOsError::GateFailed(vec![]).http_status(), 422);
    }

    #[test]
    fn http_status_unauthorized() {
        assert_eq!(SemOsError::Unauthorized("x".into()).http_status(), 403);
    }

    #[test]
    fn http_status_conflict() {
        assert_eq!(SemOsError::Conflict("x".into()).http_status(), 409);
    }

    #[test]
    fn http_status_invalid_input() {
        assert_eq!(SemOsError::InvalidInput("x".into()).http_status(), 400);
    }

    #[test]
    fn http_status_migration_pending() {
        assert_eq!(SemOsError::MigrationPending("x".into()).http_status(), 503);
    }

    #[test]
    fn http_status_internal() {
        let err = SemOsError::Internal(anyhow::anyhow!("boom"));
        assert_eq!(err.http_status(), 500);
    }

    // ── Display impl for SemOsError ──────────────────────────────

    #[test]
    fn display_not_found() {
        let e = SemOsError::NotFound("widget".into());
        assert_eq!(e.to_string(), "not found: widget");
    }

    #[test]
    fn display_gate_failed_count() {
        let v = GateViolation {
            gate_id: "G01".into(),
            severity: GateSeverity::Error,
            message: "bad".into(),
            remediation: None,
        };
        let e = SemOsError::GateFailed(vec![v.clone(), v]);
        assert_eq!(e.to_string(), "gate failed: 2 violation(s)");
    }

    #[test]
    fn display_unauthorized() {
        let e = SemOsError::Unauthorized("no token".into());
        assert_eq!(e.to_string(), "unauthorized: no token");
    }

    #[test]
    fn display_conflict() {
        let e = SemOsError::Conflict("duplicate".into());
        assert_eq!(e.to_string(), "conflict: duplicate");
    }

    #[test]
    fn display_invalid_input() {
        let e = SemOsError::InvalidInput("bad field".into());
        assert_eq!(e.to_string(), "invalid input: bad field");
    }

    #[test]
    fn display_migration_pending() {
        let e = SemOsError::MigrationPending("v42 needed".into());
        assert_eq!(e.to_string(), "migration pending — v42 needed");
    }

    #[test]
    fn display_internal() {
        let e = SemOsError::Internal(anyhow::anyhow!("segfault"));
        assert_eq!(e.to_string(), "internal: segfault");
    }

    // ── GateViolation Display ────────────────────────────────────

    #[test]
    fn gate_violation_display_error() {
        let v = GateViolation {
            gate_id: "G04".into(),
            severity: GateSeverity::Error,
            message: "proof chain broken".into(),
            remediation: Some("fix it".into()),
        };
        assert_eq!(v.to_string(), "[G04] error: proof chain broken");
    }

    #[test]
    fn gate_violation_display_warning() {
        let v = GateViolation {
            gate_id: "G02".into(),
            severity: GateSeverity::Warning,
            message: "naming convention".into(),
            remediation: None,
        };
        assert_eq!(v.to_string(), "[G02] warning: naming convention");
    }
}
