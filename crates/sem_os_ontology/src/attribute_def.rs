//! Attribute definition body types — pure value types, no DB dependency.

use serde::{Deserialize, Serialize};

use sem_os_types::{AttributeVisibility, EvidenceGrade};

fn default_attribute_evidence_grade() -> EvidenceGrade {
    EvidenceGrade::None
}

/// Body of an `attribute_def` registry snapshot.
///
/// This is the canonical definition for an attribute in SemOS — the single
/// source of truth.  Every field that the operational store
/// (`attribute_registry`) exposes **must** have a corresponding field here so
/// that the store remains a pure, switchable projection of SemOS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeDefBody {
    pub fqn: String,
    pub name: String,
    pub description: String,
    pub domain: String,
    pub data_type: AttributeDataType,
    #[serde(default = "default_attribute_evidence_grade")]
    pub evidence_grade: EvidenceGrade,
    #[serde(default)]
    pub source: Option<AttributeSource>,
    #[serde(default)]
    pub constraints: Option<AttributeConstraints>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sinks: Vec<AttributeSink>,

    // ── Fields required for full store projection ──────────────────────
    /// Classification category (identity, financial, compliance, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Business-level validation rules (JSONB in the store).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_rules: Option<serde_json::Value>,
    /// Conditional applicability rules (JSONB in the store).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applicability: Option<serde_json::Value>,
    /// Whether this attribute is required by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_required: Option<bool>,
    /// Default value (serialized as text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    /// Logical grouping identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// Whether this is a below-the-line derived attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_derived: Option<bool>,
    /// FQN of the governing derivation spec (required when `is_derived = true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation_spec_fqn: Option<String>,
    /// Whether this attribute is externally meaningful or internal/system-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<AttributeVisibility>,
}

/// Supported attribute data types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributeDataType {
    String,
    Integer,
    Decimal,
    Boolean,
    Uuid,
    Date,
    Timestamp,
    Json,
    Number,
    DateTime,
    Email,
    Phone,
    Address,
    Currency,
    Percentage,
    TaxId,
    Enum(Vec<std::string::String>),
}

impl AttributeDataType {
    /// Return the canonical string used for DB/check/value-type interoperability.
    ///
    /// # Examples
    ///
    /// ```
    /// use sem_os_core::attribute_def::AttributeDataType;
    ///
    /// assert_eq!(AttributeDataType::String.to_pg_check_value(), "string");
    /// assert_eq!(AttributeDataType::TaxId.to_pg_check_value(), "tax_id");
    /// ```
    pub fn to_pg_check_value(&self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Decimal => "decimal",
            Self::Boolean => "boolean",
            Self::Uuid => "uuid",
            Self::Date => "date",
            Self::Timestamp => "timestamp",
            Self::Json => "json",
            Self::Enum(_) => "enum",
            Self::Number => "number",
            Self::DateTime => "datetime",
            Self::Email => "email",
            Self::Phone => "phone",
            Self::Address => "address",
            Self::Currency => "currency",
            Self::Percentage => "percentage",
            Self::TaxId => "tax_id",
        }
    }

    /// Parse a DB/check/value-type string into the canonical enum.
    ///
    /// # Examples
    ///
    /// ```
    /// use sem_os_core::attribute_def::AttributeDataType;
    ///
    /// assert_eq!(
    ///     AttributeDataType::from_pg_check_value("datetime"),
    ///     Some(AttributeDataType::DateTime)
    /// );
    /// assert_eq!(
    ///     AttributeDataType::from_pg_check_value("tax_id"),
    ///     Some(AttributeDataType::TaxId)
    /// );
    /// ```
    pub fn from_pg_check_value(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "string" | "text" | "varchar" | "character_varying" => Some(Self::String),
            "integer" | "int" | "bigint" | "smallint" => Some(Self::Integer),
            "decimal" | "numeric" => Some(Self::Decimal),
            "number" | "float" | "double" | "real" => Some(Self::Number),
            "boolean" | "bool" => Some(Self::Boolean),
            "uuid" => Some(Self::Uuid),
            "date" => Some(Self::Date),
            "timestamp" => Some(Self::Timestamp),
            "datetime" | "date_time" | "timestamp_tz" => Some(Self::DateTime),
            "json" | "jsonb" => Some(Self::Json),
            "email" => Some(Self::Email),
            "phone" => Some(Self::Phone),
            "address" => Some(Self::Address),
            "currency" => Some(Self::Currency),
            "percentage" | "percent" => Some(Self::Percentage),
            "tax_id" | "taxid" => Some(Self::TaxId),
            _ => None,
        }
    }
}

/// Where an attribute value comes from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeSource {
    #[serde(default)]
    pub producing_verb: Option<String>,
    /// DB schema where the canonical value lives (e.g. "ob-poc", "sem_reg")
    #[serde(default)]
    pub schema: Option<String>,
    /// DB table where the canonical value lives (e.g. "cbus", "entities")
    #[serde(default)]
    pub table: Option<String>,
    /// DB column name (e.g. "jurisdiction_code")
    #[serde(default)]
    pub column: Option<String>,
    #[serde(default)]
    pub derived: bool,
}

/// Validation constraints on attribute values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeConstraints {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub unique: bool,
    #[serde(default)]
    pub min_length: Option<usize>,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub pattern: Option<String>,
    #[serde(default)]
    pub valid_values: Option<Vec<String>>,
}

/// A consuming verb that reads this attribute as an argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeSink {
    pub consuming_verb: String,
    pub arg_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_round_trip() {
        let val = AttributeDefBody {
            fqn: "cbu.jurisdiction_code".into(),
            name: "jurisdiction_code".into(),
            description: "ISO jurisdiction".into(),
            domain: "cbu".into(),
            data_type: AttributeDataType::Enum(vec!["LU".into(), "IE".into(), "DE".into()]),
            evidence_grade: EvidenceGrade::None,
            source: Some(AttributeSource {
                producing_verb: Some("cbu.create".into()),
                schema: Some("ob-poc".into()),
                table: Some("cbus".into()),
                column: Some("jurisdiction_code".into()),
                derived: false,
            }),
            constraints: Some(AttributeConstraints {
                required: true,
                unique: false,
                min_length: Some(2),
                max_length: Some(2),
                pattern: Some("^[A-Z]{2}$".into()),
                valid_values: Some(vec!["LU".into(), "IE".into()]),
            }),
            sinks: vec![AttributeSink {
                consuming_verb: "session.load-jurisdiction".into(),
                arg_name: "code".into(),
            }],
            category: Some("entity".into()),
            validation_rules: Some(serde_json::json!({"min_length": 2})),
            applicability: Some(serde_json::json!({"entity_types": ["cbu"]})),
            is_required: Some(true),
            default_value: None,
            group_id: Some("jurisdiction".into()),
            is_derived: Some(false),
            derivation_spec_fqn: None,
            visibility: None,
        };
        let json = serde_json::to_value(&val).unwrap();
        // Check Enum variant serialization
        assert!(json["data_type"]["enum"].is_array());
        // Check #[serde(default)] on optional fields: omitting them deserializes fine
        let minimal: AttributeDefBody = serde_json::from_str(
            r#"{"fqn":"x","name":"x","description":"x","domain":"x","data_type":"string"}"#,
        )
        .unwrap();
        assert_eq!(minimal.evidence_grade, EvidenceGrade::None);
        assert!(minimal.source.is_none());
        assert!(minimal.sinks.is_empty());
        // Round-trip
        let back: AttributeDefBody = serde_json::from_value(json.clone()).unwrap();
        let json2 = serde_json::to_value(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn attribute_data_type_pg_value_round_trip() {
        let samples = [
            AttributeDataType::String,
            AttributeDataType::Integer,
            AttributeDataType::Decimal,
            AttributeDataType::Boolean,
            AttributeDataType::Uuid,
            AttributeDataType::Date,
            AttributeDataType::Timestamp,
            AttributeDataType::Json,
            AttributeDataType::Number,
            AttributeDataType::DateTime,
            AttributeDataType::Email,
            AttributeDataType::Phone,
            AttributeDataType::Address,
            AttributeDataType::Currency,
            AttributeDataType::Percentage,
            AttributeDataType::TaxId,
        ];

        for sample in samples {
            let pg = sample.to_pg_check_value();
            assert_eq!(AttributeDataType::from_pg_check_value(pg), Some(sample));
        }
        assert_eq!(
            AttributeDataType::from_pg_check_value("date_time"),
            Some(AttributeDataType::DateTime)
        );
    }
}
