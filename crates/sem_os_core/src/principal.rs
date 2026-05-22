use std::collections::HashMap;

use crate::error::SemOsError;
use sem_os_types::agent_mode::AgentMode;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Principal {
    pub actor_id: String,
    pub roles: Vec<String>,
    pub claims: HashMap<String, String>,
    pub tenancy: Option<String>,
}

impl Principal {
    /// Construct from validated JWT claims at the server boundary (remote mode).
    /// The server middleware calls this; core logic never reads raw JWT tokens.
    pub fn from_jwt_claims(claims: &JwtClaims) -> Result<Self, SemOsError> {
        let actor_id = claims
            .sub
            .clone()
            .ok_or_else(|| SemOsError::Unauthorized("missing sub claim".into()))?;
        Ok(Self {
            actor_id,
            roles: claims.roles.clone().unwrap_or_default(),
            claims: claims.extra.clone().unwrap_or_default(),
            tenancy: claims.tenancy.clone(),
        })
    }

    /// Construct explicitly for in-process mode.
    /// Caller is responsible for populating roles correctly.
    /// There is no implicit or thread-local identity anywhere in the codebase.
    pub fn in_process(actor_id: impl Into<String>, roles: Vec<String>) -> Self {
        Self {
            actor_id: actor_id.into(),
            roles,
            claims: HashMap::new(),
            tenancy: None,
        }
    }

    /// System principal for internal/automated operations.
    pub fn system() -> Self {
        Self::in_process("system", vec!["admin".to_string()])
    }

    /// Construct with explicit actor_id and roles (shorthand for in_process).
    pub fn explicit(actor_id: impl Into<String>, roles: Vec<String>) -> Self {
        Self::in_process(actor_id, roles)
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    pub fn is_admin(&self) -> bool {
        self.has_role("admin")
    }

    pub fn require_admin(&self) -> Result<(), SemOsError> {
        if self.is_admin() {
            Ok(())
        } else {
            Err(SemOsError::Unauthorized(format!(
                "{} is not an admin",
                self.actor_id
            )))
        }
    }

    /// Read agent mode from JWT claims.
    /// Accepts "research" or "governed" (case-insensitive).
    /// Defaults to `AgentMode::Governed` if claim is missing or unrecognised.
    pub fn agent_mode(&self) -> AgentMode {
        self.claims
            .get("agent_mode")
            .and_then(|v| AgentMode::parse(v))
            .unwrap_or(AgentMode::Governed)
    }
}

/// JWT claims shape expected from the identity provider.
/// Deserialised by the server JWT middleware.
#[derive(Debug, serde::Deserialize)]
pub struct JwtClaims {
    pub sub: Option<String>,
    pub roles: Option<Vec<String>>,
    pub tenancy: Option<String>,
    #[serde(flatten)]
    pub extra: Option<HashMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims_with_sub(sub: &str) -> JwtClaims {
        JwtClaims {
            sub: Some(sub.to_string()),
            roles: Some(vec!["analyst".into()]),
            tenancy: Some("tenant-a".into()),
            extra: Some(HashMap::from([("foo".into(), "bar".into())])),
        }
    }

    #[test]
    fn from_jwt_claims_happy_path() {
        let claims = claims_with_sub("alice");
        let p = Principal::from_jwt_claims(&claims).unwrap();
        assert_eq!(p.actor_id, "alice");
        assert_eq!(p.roles, vec!["analyst"]);
        assert_eq!(p.tenancy, Some("tenant-a".into()));
        assert_eq!(p.claims.get("foo").unwrap(), "bar");
    }

    #[test]
    fn from_jwt_claims_missing_sub() {
        let claims = JwtClaims {
            sub: None,
            roles: Some(vec![]),
            tenancy: None,
            extra: None,
        };
        let err = Principal::from_jwt_claims(&claims).unwrap_err();
        assert!(matches!(err, SemOsError::Unauthorized(_)));
    }

    #[test]
    fn from_jwt_claims_defaults() {
        let claims = JwtClaims {
            sub: Some("bob".into()),
            roles: None,
            tenancy: None,
            extra: None,
        };
        let p = Principal::from_jwt_claims(&claims).unwrap();
        assert_eq!(p.actor_id, "bob");
        assert!(p.roles.is_empty());
        assert!(p.claims.is_empty());
        assert!(p.tenancy.is_none());
    }

    #[test]
    fn in_process_constructs_correctly() {
        let p = Principal::in_process("system", vec!["admin".into()]);
        assert_eq!(p.actor_id, "system");
        assert_eq!(p.roles, vec!["admin"]);
        assert!(p.claims.is_empty());
        assert!(p.tenancy.is_none());
    }

    #[test]
    fn has_role_present_and_absent() {
        let p = Principal::in_process("u", vec!["viewer".into(), "admin".into()]);
        assert!(p.has_role("admin"));
        assert!(p.has_role("viewer"));
        assert!(!p.has_role("superuser"));
    }

    #[test]
    fn is_admin_true_when_present() {
        let p = Principal::in_process("u", vec!["admin".into()]);
        assert!(p.is_admin());
    }

    #[test]
    fn is_admin_false_when_absent() {
        let p = Principal::in_process("u", vec!["viewer".into()]);
        assert!(!p.is_admin());
    }

    #[test]
    fn require_admin_ok_when_admin() {
        let p = Principal::in_process("u", vec!["admin".into()]);
        assert!(p.require_admin().is_ok());
    }

    #[test]
    fn require_admin_err_when_not_admin() {
        let p = Principal::in_process("u", vec!["viewer".into()]);
        let err = p.require_admin().unwrap_err();
        assert!(matches!(err, SemOsError::Unauthorized(_)));
    }

    #[test]
    fn agent_mode_defaults_to_governed() {
        let p = Principal::in_process("u", vec![]);
        assert_eq!(p.agent_mode(), AgentMode::Governed);
    }

    #[test]
    fn agent_mode_research_from_claim() {
        let claims = JwtClaims {
            sub: Some("u".into()),
            roles: None,
            tenancy: None,
            extra: Some(HashMap::from([("agent_mode".into(), "research".into())])),
        };
        let p = Principal::from_jwt_claims(&claims).unwrap();
        assert_eq!(p.agent_mode(), AgentMode::Research);
    }

    #[test]
    fn agent_mode_governed_from_claim() {
        let claims = JwtClaims {
            sub: Some("u".into()),
            roles: None,
            tenancy: None,
            extra: Some(HashMap::from([("agent_mode".into(), "governed".into())])),
        };
        let p = Principal::from_jwt_claims(&claims).unwrap();
        assert_eq!(p.agent_mode(), AgentMode::Governed);
    }
}
