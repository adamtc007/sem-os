//! Domain Pack contract for configuration-native state-machine agents.
//!
//! This is distinct from Journey Pack manifests. A Domain Pack declares the
//! state-machine surface an adapter may discover, dry-run, and eventually mutate.

use crate::acp_projection::AcpProjectionKind;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainPackManifest {
    pub pack_id: String,
    pub name: String,
    pub version: String,
    pub implementation_mode: PackImplementationMode,
    pub compatibility_tier: PackCompatibilityTier,
    #[serde(default)]
    pub owned_dags: Vec<String>,
    #[serde(default)]
    pub owned_packs: Vec<String>,
    #[serde(default)]
    pub owned_state_machines: Vec<String>,
    #[serde(default)]
    pub owned_constellation_maps: Vec<String>,
    #[serde(default)]
    pub owned_constellation_families: Vec<String>,
    #[serde(default)]
    pub owned_universes: Vec<String>,
    #[serde(default)]
    pub owned_verb_prefixes: Vec<String>,
    #[serde(default)]
    pub owned_entity_kinds: Vec<String>,
    #[serde(default)]
    pub business_crates: Vec<String>,
    #[serde(default)]
    pub owned_constellations: Vec<String>,
    #[serde(default)]
    pub allowed_transitions: Vec<DomainTransition>,
    #[serde(default)]
    pub discovery_probes: Vec<DiscoveryProbe>,
    #[serde(default)]
    pub projection_catalog: Vec<ProjectionCatalogEntry>,
    #[serde(default)]
    pub mention_namespaces: Vec<MentionNamespace>,
    #[serde(default)]
    pub declared_modes: Vec<DeclaredMode>,
    #[serde(default)]
    pub workflow_phases: Vec<WorkflowPhase>,
    #[serde(default)]
    pub acp_personas: Vec<AcpPersonaDeclaration>,
    #[serde(default)]
    pub resource_uri_schemes: Vec<ResourceUriScheme>,
    #[serde(default)]
    pub external_mcp_transports: Vec<ExternalMcpTransport>,
    #[serde(default)]
    pub typed_extension_points: Vec<TypedExtensionPoint>,
    pub classification_policy: ContextClassificationPolicy,
}

impl DomainPackManifest {
    pub fn validate(&self) -> DomainPackValidationReport {
        validate_domain_pack(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainPackTaxonomyReload {
    pub manifest: DomainPackManifest,
    pub surface_hash: String,
    pub surfaces: BTreeMap<String, DomainPackTaxonomySurface>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainPackTaxonomySurface {
    pub path: PathBuf,
    pub canonical_payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainPackReloadIndexEntry {
    pub pack_id: String,
    pub source_fingerprints: Vec<DomainPackSourceFingerprint>,
    pub surface_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_set_id: Option<Uuid>,
    pub last_checked_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_loaded_at: Option<DateTime<Utc>>,
    pub status: DomainPackReloadStatus,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainPackSourceFingerprint {
    pub surface: String,
    pub path: String,
    pub size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_unix_millis: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainPackReloadStatus {
    Clean,
    Loaded,
    IndexOnly,
    PublishRequired,
}

impl DomainPackReloadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Loaded => "loaded",
            Self::IndexOnly => "index_only",
            Self::PublishRequired => "publish_required",
        }
    }
}

impl std::str::FromStr for DomainPackReloadStatus {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "clean" => Ok(Self::Clean),
            "loaded" => Ok(Self::Loaded),
            "index_only" => Ok(Self::IndexOnly),
            "publish_required" => Ok(Self::PublishRequired),
            other => bail!("unknown domain pack reload status {other}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainPackRefreshPlan {
    pub pack_id: String,
    pub action: DomainPackRefreshAction,
    pub reason: String,
    pub reload: Option<DomainPackTaxonomyReload>,
    pub index_entry: DomainPackReloadIndexEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainPackRefreshAction {
    Skip,
    IndexOnly,
    PublishRequired,
}

pub fn reload_domain_pack_taxonomy_from_yaml(
    config_root: impl AsRef<Path>,
    pack_id: &str,
) -> Result<DomainPackTaxonomyReload> {
    let config_root = config_root.as_ref();
    let (domain_pack_path, domain_pack_yaml) = find_yaml_by_field(
        &config_root.join("sem_os_seeds/domain_packs"),
        &["pack_id"],
        pack_id,
    )?;
    let manifest: DomainPackManifest = serde_yaml::from_value(domain_pack_yaml.clone())
        .with_context(|| format!("failed to parse domain pack manifest {pack_id}"))?;

    let report = manifest.validate();
    if !report.valid {
        bail!(
            "domain pack {pack_id} failed validation: {:?}",
            report.diagnostics
        );
    }

    let mut surfaces = BTreeMap::new();
    insert_surface(
        &mut surfaces,
        format!("domain_pack:{pack_id}"),
        domain_pack_path,
        &domain_pack_yaml,
    )?;

    for dag_id in &manifest.owned_dags {
        let (path, yaml) = find_yaml_by_field(
            &config_root.join("sem_os_seeds/dag_taxonomies"),
            &["dag_id", "workspace"],
            dag_id,
        )?;
        insert_surface(&mut surfaces, format!("dag:{dag_id}"), path, &yaml)?;
    }

    for pack_id in &manifest.owned_packs {
        let (path, yaml) = find_yaml_by_field(&config_root.join("packs"), &["id"], pack_id)?;
        insert_surface(&mut surfaces, format!("pack:{pack_id}"), path, &yaml)?;

        for macro_fqn in owned_pack_macro_fqns(config_root, pack_id)? {
            let (path, yaml) =
                find_macro_yaml_by_fqn(&config_root.join("verb_schemas/macros"), &macro_fqn)?;
            insert_surface(&mut surfaces, format!("macro:{macro_fqn}"), path, &yaml)?;
        }
    }

    for state_machine in &manifest.owned_state_machines {
        let (path, yaml) = find_yaml_by_field(
            &config_root.join("sem_os_seeds/state_machines"),
            &["state_machine"],
            state_machine,
        )?;
        insert_surface(
            &mut surfaces,
            format!("state_machine:{state_machine}"),
            path,
            &yaml,
        )?;
    }

    for constellation in &manifest.owned_constellation_maps {
        let (path, yaml) = find_yaml_by_field(
            &config_root.join("sem_os_seeds/constellation_maps"),
            &["constellation"],
            constellation,
        )?;
        insert_surface(
            &mut surfaces,
            format!("constellation_map:{constellation}"),
            path,
            &yaml,
        )?;
    }

    for family in &manifest.owned_constellation_families {
        let (path, yaml) = find_yaml_by_field(
            &config_root.join("sem_os_seeds/constellation_families"),
            &["family_id", "fqn"],
            family,
        )?;
        insert_surface(
            &mut surfaces,
            format!("constellation_family:{family}"),
            path,
            &yaml,
        )?;
    }

    for universe in &manifest.owned_universes {
        let (path, yaml) = find_yaml_by_field(
            &config_root.join("sem_os_seeds/universes"),
            &["fqn", "universe_id"],
            universe,
        )?;
        insert_surface(&mut surfaces, format!("universe:{universe}"), path, &yaml)?;
    }

    let entity_taxonomy_path = config_root.join("ontology/entity_taxonomy.yaml");
    let entity_taxonomy = parse_yaml_file(&entity_taxonomy_path)?;
    validate_owned_entity_kinds(&entity_taxonomy, &manifest.owned_entity_kinds)?;
    insert_surface(
        &mut surfaces,
        "ontology:entity_taxonomy".to_string(),
        entity_taxonomy_path,
        &entity_taxonomy,
    )?;

    let surface_hash = hash_surfaces(&surfaces);
    Ok(DomainPackTaxonomyReload {
        manifest,
        surface_hash,
        surfaces,
    })
}

pub fn reload_all_domain_pack_taxonomies_from_yaml(
    config_root: impl AsRef<Path>,
) -> Result<BTreeMap<String, DomainPackTaxonomyReload>> {
    let config_root = config_root.as_ref();
    let mut out = BTreeMap::new();
    for path in yaml_files(&config_root.join("sem_os_seeds/domain_packs"))? {
        let yaml = parse_yaml_file(&path)?;
        let Some(pack_id) = yaml_field(&yaml, "pack_id").and_then(serde_yaml::Value::as_str) else {
            bail!("domain pack {} does not declare pack_id", path.display());
        };
        let reload = reload_domain_pack_taxonomy_from_yaml(config_root, pack_id)?;
        if out.insert(pack_id.to_string(), reload).is_some() {
            bail!("duplicate domain pack id {pack_id}");
        }
    }
    Ok(out)
}

pub fn refresh_domain_pack_taxonomy_with_index(
    config_root: impl AsRef<Path>,
    pack_id: &str,
    previous: Option<&DomainPackReloadIndexEntry>,
    force_check: bool,
    now: DateTime<Utc>,
) -> Result<DomainPackRefreshPlan> {
    let config_root = config_root.as_ref();

    if !force_check {
        if let Some(previous) = previous {
            let probe = probe_domain_pack_sources(config_root, &previous.source_fingerprints)?;
            if probe.clean {
                let mut index_entry = previous.clone();
                index_entry.last_checked_at = now;
                index_entry.status = DomainPackReloadStatus::Clean;
                index_entry.diagnostics.clear();
                return Ok(DomainPackRefreshPlan {
                    pack_id: pack_id.to_string(),
                    action: DomainPackRefreshAction::Skip,
                    reason: "source fingerprints unchanged".to_string(),
                    reload: None,
                    index_entry,
                });
            }
        }
    }

    let reload = reload_domain_pack_taxonomy_from_yaml(config_root, pack_id)?;
    let hash_changed = previous
        .map(|entry| entry.surface_hash != reload.surface_hash)
        .unwrap_or(true);
    let action = if hash_changed {
        DomainPackRefreshAction::PublishRequired
    } else {
        DomainPackRefreshAction::IndexOnly
    };
    let status = if hash_changed {
        DomainPackReloadStatus::PublishRequired
    } else {
        DomainPackReloadStatus::IndexOnly
    };
    let reason = match (previous.is_some(), force_check, hash_changed) {
        (false, _, _) => "no prior reload index".to_string(),
        (true, true, true) => "forced check found content hash change".to_string(),
        (true, true, false) => "forced check found unchanged content hash".to_string(),
        (true, false, true) => "source fingerprint changed and content hash changed".to_string(),
        (true, false, false) => {
            "source fingerprint changed but content hash is unchanged".to_string()
        }
    };
    let mut index_entry = reload_index_entry_from_reload(config_root, &reload, now, status)?;
    index_entry.last_loaded_at = previous.and_then(|entry| entry.last_loaded_at);

    Ok(DomainPackRefreshPlan {
        pack_id: pack_id.to_string(),
        action,
        reason,
        reload: Some(reload),
        index_entry,
    })
}

pub fn reload_index_entry_from_reload(
    config_root: impl AsRef<Path>,
    reload: &DomainPackTaxonomyReload,
    now: DateTime<Utc>,
    status: DomainPackReloadStatus,
) -> Result<DomainPackReloadIndexEntry> {
    Ok(DomainPackReloadIndexEntry {
        pack_id: reload.manifest.pack_id.clone(),
        source_fingerprints: source_fingerprints(config_root.as_ref(), &reload.surfaces)?,
        surface_hash: reload.surface_hash.clone(),
        snapshot_set_id: None,
        last_checked_at: now,
        last_loaded_at: (status == DomainPackReloadStatus::Loaded).then_some(now),
        status,
        diagnostics: Vec::new(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceProbe {
    clean: bool,
}

fn probe_domain_pack_sources(
    config_root: &Path,
    previous: &[DomainPackSourceFingerprint],
) -> Result<SourceProbe> {
    if previous.is_empty() {
        return Ok(SourceProbe { clean: false });
    }

    for old in previous {
        let path = source_path(config_root, &old.path);
        let current = source_fingerprint_for_path(config_root, &old.surface, &path)?;
        if current.size_bytes != old.size_bytes
            || current.modified_unix_millis != old.modified_unix_millis
        {
            return Ok(SourceProbe { clean: false });
        }
    }

    Ok(SourceProbe { clean: true })
}

fn source_fingerprints(
    config_root: &Path,
    surfaces: &BTreeMap<String, DomainPackTaxonomySurface>,
) -> Result<Vec<DomainPackSourceFingerprint>> {
    surfaces
        .iter()
        .map(|(surface, value)| source_fingerprint_for_path(config_root, surface, &value.path))
        .collect()
}

fn source_fingerprint_for_path(
    config_root: &Path,
    surface: &str,
    path: &Path,
) -> Result<DomainPackSourceFingerprint> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    let modified_unix_millis = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64);
    Ok(DomainPackSourceFingerprint {
        surface: surface.to_string(),
        path: stable_source_path(config_root, path),
        size_bytes: metadata.len(),
        modified_unix_millis,
    })
}

fn source_path(config_root: &Path, stored: &str) -> PathBuf {
    let path = PathBuf::from(stored);
    if path.is_absolute() {
        path
    } else {
        config_root.join(path)
    }
}

fn stable_source_path(config_root: &Path, path: &Path) -> String {
    let root = fs::canonicalize(config_root).unwrap_or_else(|_| config_root.to_path_buf());
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.strip_prefix(&root)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackImplementationMode {
    NativeCompiled,
    ExternalAdapter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackCompatibilityTier {
    DryRunOnly,
    ReferenceMutation,
    ReuseProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainTransition {
    pub transition_ref: String,
    pub entity_type: String,
    pub state_machine: String,
    pub verb: String,
    pub from_state: String,
    pub to_state: String,
    #[serde(default)]
    pub dry_run_enabled: bool,
    #[serde(default)]
    pub mutation_enabled: bool,
    #[serde(default)]
    pub hitl_required: bool,
    #[serde(default)]
    pub evidence_refs_required: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryProbe {
    pub probe_id: String,
    pub operation: String,
    pub target: String,
    #[serde(default)]
    pub idempotent: bool,
    #[serde(default)]
    pub modeled: bool,
    #[serde(default)]
    pub first_class_state_mutation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectionCatalogEntry {
    pub kind: AcpProjectionKind,
    pub source: String,
    pub default_classification: ClassificationLimit,
    #[serde(default)]
    pub allowed_subject_kinds: Vec<String>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub acp_visible_by_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MentionNamespace {
    pub namespace: String,
    pub target_kind: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredMode {
    pub mode_id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPhase {
    pub phase_id: String,
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpPersonaDeclaration {
    pub persona_id: String,
    pub label: String,
    pub description: String,
    #[serde(default)]
    pub mutation_authority: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceUriScheme {
    pub scheme: String,
    pub resource_kind: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalMcpTransport {
    pub server_id: String,
    pub description: String,
    pub read_only: bool,
    pub classification: ClassificationLimit,
    #[serde(default)]
    pub allowed_probe_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedExtensionPoint {
    pub extension_id: String,
    pub extension_kind: String,
    pub implementation_ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryRequest {
    pub pack_id: String,
    pub probe_id: String,
    pub subject: DiscoverySubject,
    #[serde(default)]
    pub context: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoverySubject {
    pub subject_kind: String,
    pub subject_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryResponse {
    pub probe_id: String,
    pub subject: DiscoverySubject,
    #[serde(default)]
    pub observations: Vec<DiscoveryObservation>,
    #[serde(default)]
    pub provenance: Vec<DiscoveryProvenance>,
    #[serde(default)]
    pub first_class_state_mutated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryObservation {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(default)]
    pub classification: Option<ClassificationLimit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryProvenance {
    pub source: String,
    #[serde(default)]
    pub snapshot_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscoveryAuthorizationError {
    PackMismatch { expected: String, actual: String },
    UnknownProbe { probe_id: String },
    UnsafeProbe { probe_id: String, code: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextClassificationPolicy {
    pub max_prompt_classification: ClassificationLimit,
    #[serde(default)]
    pub allow_external_llm: bool,
    #[serde(default)]
    pub required_redactions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationLimit {
    Public,
    Internal,
    Confidential,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainPackValidationReport {
    pub valid: bool,
    pub diagnostics: Vec<DomainPackDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainPackDiagnostic {
    pub code: String,
    pub message: String,
}

impl DomainPackValidationReport {
    fn from_diagnostics(diagnostics: Vec<DomainPackDiagnostic>) -> Self {
        Self {
            valid: diagnostics.is_empty(),
            diagnostics,
        }
    }
}

pub fn validate_domain_pack(manifest: &DomainPackManifest) -> DomainPackValidationReport {
    let mut diagnostics = Vec::new();

    require_non_empty(&mut diagnostics, "pack_id", &manifest.pack_id);
    require_non_empty(&mut diagnostics, "name", &manifest.name);
    require_non_empty(&mut diagnostics, "version", &manifest.version);

    validate_owned_values("owned_dags", &manifest.owned_dags, &mut diagnostics);
    validate_owned_values("owned_packs", &manifest.owned_packs, &mut diagnostics);
    validate_owned_values(
        "owned_state_machines",
        &manifest.owned_state_machines,
        &mut diagnostics,
    );
    validate_owned_values(
        "owned_constellation_maps",
        &manifest.owned_constellation_maps,
        &mut diagnostics,
    );
    validate_owned_values(
        "owned_constellation_families",
        &manifest.owned_constellation_families,
        &mut diagnostics,
    );
    validate_owned_values(
        "owned_universes",
        &manifest.owned_universes,
        &mut diagnostics,
    );
    validate_owned_values(
        "owned_verb_prefixes",
        &manifest.owned_verb_prefixes,
        &mut diagnostics,
    );
    validate_owned_values(
        "owned_entity_kinds",
        &manifest.owned_entity_kinds,
        &mut diagnostics,
    );
    validate_owned_values(
        "business_crates",
        &manifest.business_crates,
        &mut diagnostics,
    );
    validate_owned_values(
        "owned_constellations",
        &manifest.owned_constellations,
        &mut diagnostics,
    );

    if manifest.owned_constellations.is_empty() {
        diagnostics.push(diagnostic(
            "domain_pack.no_owned_constellations",
            "domain pack must declare at least one owned constellation",
        ));
    }

    if manifest.allowed_transitions.is_empty() {
        diagnostics.push(diagnostic(
            "domain_pack.no_allowed_transitions",
            "domain pack must declare at least one allowed transition",
        ));
    }

    let mut transition_refs = HashSet::new();
    for transition in &manifest.allowed_transitions {
        validate_transition(transition, &mut transition_refs, &mut diagnostics);
    }

    let mut probe_ids = HashSet::new();
    for probe in &manifest.discovery_probes {
        validate_probe(probe, &mut probe_ids, &mut diagnostics);
    }

    let mut projection_kinds = HashSet::new();
    for projection in &manifest.projection_catalog {
        validate_projection(projection, &mut projection_kinds, &mut diagnostics);
    }

    let mut mention_namespaces = HashSet::new();
    for namespace in &manifest.mention_namespaces {
        validate_mention_namespace(namespace, &mut mention_namespaces, &mut diagnostics);
    }

    let mut mode_ids = HashSet::new();
    for mode in &manifest.declared_modes {
        validate_declared_mode(mode, &mut mode_ids, &mut diagnostics);
    }

    let mut workflow_phases = HashSet::new();
    for phase in &manifest.workflow_phases {
        validate_workflow_phase(phase, &mut workflow_phases, &mut diagnostics);
    }

    let mut personas = HashSet::new();
    for persona in &manifest.acp_personas {
        validate_acp_persona(persona, &mut personas, &mut diagnostics);
    }

    let mut uri_schemes = HashSet::new();
    for scheme in &manifest.resource_uri_schemes {
        validate_resource_uri_scheme(scheme, &mut uri_schemes, &mut diagnostics);
    }

    let probe_ids: HashSet<&str> = manifest
        .discovery_probes
        .iter()
        .map(|probe| probe.probe_id.as_str())
        .collect();
    for transport in &manifest.external_mcp_transports {
        validate_external_mcp_transport(transport, &probe_ids, &mut diagnostics);
    }

    if matches!(
        manifest.compatibility_tier,
        PackCompatibilityTier::DryRunOnly | PackCompatibilityTier::ReuseProof
    ) {
        for transition in &manifest.allowed_transitions {
            if transition.mutation_enabled {
                diagnostics.push(diagnostic(
                    "domain_pack.mutation_not_allowed_for_tier",
                    format!(
                        "{} enables mutation but pack tier is {:?}",
                        transition.transition_ref, manifest.compatibility_tier
                    ),
                ));
            }
        }
    }

    if manifest.classification_policy.allow_external_llm
        && matches!(
            manifest.classification_policy.max_prompt_classification,
            ClassificationLimit::Confidential | ClassificationLimit::Restricted
        )
    {
        diagnostics.push(diagnostic(
            "domain_pack.external_llm_classification_limit",
            "external LLM prompts may not include confidential or restricted context",
        ));
    }

    DomainPackValidationReport::from_diagnostics(diagnostics)
}

pub fn authorize_discovery_probe<'a>(
    manifest: &'a DomainPackManifest,
    request: &DiscoveryRequest,
) -> Result<&'a DiscoveryProbe, DiscoveryAuthorizationError> {
    if request.pack_id != manifest.pack_id {
        return Err(DiscoveryAuthorizationError::PackMismatch {
            expected: manifest.pack_id.clone(),
            actual: request.pack_id.clone(),
        });
    }

    let Some(probe) = manifest
        .discovery_probes
        .iter()
        .find(|probe| probe.probe_id == request.probe_id)
    else {
        return Err(DiscoveryAuthorizationError::UnknownProbe {
            probe_id: request.probe_id.clone(),
        });
    };

    if !probe.idempotent {
        return Err(DiscoveryAuthorizationError::UnsafeProbe {
            probe_id: probe.probe_id.clone(),
            code: "domain_pack.probe_not_idempotent".to_string(),
        });
    }

    if !probe.modeled {
        return Err(DiscoveryAuthorizationError::UnsafeProbe {
            probe_id: probe.probe_id.clone(),
            code: "domain_pack.probe_not_modeled".to_string(),
        });
    }

    if probe.first_class_state_mutation {
        return Err(DiscoveryAuthorizationError::UnsafeProbe {
            probe_id: probe.probe_id.clone(),
            code: "domain_pack.probe_mutates_state".to_string(),
        });
    }

    Ok(probe)
}

fn validate_transition(
    transition: &DomainTransition,
    seen: &mut HashSet<String>,
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    require_non_empty(diagnostics, "transition_ref", &transition.transition_ref);
    require_non_empty(diagnostics, "entity_type", &transition.entity_type);
    require_non_empty(diagnostics, "state_machine", &transition.state_machine);
    require_non_empty(diagnostics, "verb", &transition.verb);
    require_non_empty(diagnostics, "from_state", &transition.from_state);
    require_non_empty(diagnostics, "to_state", &transition.to_state);

    if !transition.transition_ref.trim().is_empty()
        && !seen.insert(transition.transition_ref.clone())
    {
        diagnostics.push(diagnostic(
            "domain_pack.duplicate_transition_ref",
            format!("duplicate transition_ref {}", transition.transition_ref),
        ));
    }

    if transition.from_state == transition.to_state {
        diagnostics.push(diagnostic(
            "domain_pack.noop_transition",
            format!(
                "{} has the same from_state and to_state",
                transition.transition_ref
            ),
        ));
    }

    if transition.mutation_enabled && !transition.hitl_required {
        diagnostics.push(diagnostic(
            "domain_pack.mutation_requires_hitl",
            format!(
                "{} enables mutation without HITL",
                transition.transition_ref
            ),
        ));
    }

    if transition.mutation_enabled && !transition.dry_run_enabled {
        diagnostics.push(diagnostic(
            "domain_pack.mutation_requires_dry_run",
            format!(
                "{} enables mutation without dry-run",
                transition.transition_ref
            ),
        ));
    }
}

fn validate_probe(
    probe: &DiscoveryProbe,
    seen: &mut HashSet<String>,
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    require_non_empty(diagnostics, "probe_id", &probe.probe_id);
    require_non_empty(diagnostics, "operation", &probe.operation);
    require_non_empty(diagnostics, "target", &probe.target);

    if !probe.probe_id.trim().is_empty() && !seen.insert(probe.probe_id.clone()) {
        diagnostics.push(diagnostic(
            "domain_pack.duplicate_probe_id",
            format!("duplicate probe_id {}", probe.probe_id),
        ));
    }

    if !probe.idempotent {
        diagnostics.push(diagnostic(
            "domain_pack.probe_not_idempotent",
            format!("{} must be idempotent", probe.probe_id),
        ));
    }

    if !probe.modeled {
        diagnostics.push(diagnostic(
            "domain_pack.probe_not_modeled",
            format!("{} must be modeled", probe.probe_id),
        ));
    }

    if probe.first_class_state_mutation {
        diagnostics.push(diagnostic(
            "domain_pack.probe_mutates_state",
            format!("{} declares first-class state mutation", probe.probe_id),
        ));
    }
}

fn validate_projection(
    projection: &ProjectionCatalogEntry,
    seen: &mut HashSet<AcpProjectionKind>,
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    require_non_empty(diagnostics, "projection.source", &projection.source);

    if !seen.insert(projection.kind) {
        diagnostics.push(diagnostic(
            "domain_pack.duplicate_projection_kind",
            format!("duplicate projection kind {}", projection.kind.as_str()),
        ));
    }

    if matches!(projection.max_depth, Some(0)) {
        diagnostics.push(diagnostic(
            "domain_pack.invalid_projection_depth",
            format!("{} has max_depth=0", projection.kind.as_str()),
        ));
    }
}

fn validate_mention_namespace(
    namespace: &MentionNamespace,
    seen: &mut HashSet<String>,
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    require_non_empty(diagnostics, "mention.namespace", &namespace.namespace);
    require_non_empty(diagnostics, "mention.target_kind", &namespace.target_kind);
    require_non_empty(diagnostics, "mention.description", &namespace.description);
    if !namespace.namespace.trim().is_empty() && !seen.insert(namespace.namespace.clone()) {
        diagnostics.push(diagnostic(
            "domain_pack.duplicate_mention_namespace",
            format!("duplicate mention namespace {}", namespace.namespace),
        ));
    }
}

fn validate_declared_mode(
    mode: &DeclaredMode,
    seen: &mut HashSet<String>,
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    require_non_empty(diagnostics, "mode.mode_id", &mode.mode_id);
    require_non_empty(diagnostics, "mode.label", &mode.label);
    require_non_empty(diagnostics, "mode.description", &mode.description);
    if !mode.mode_id.trim().is_empty() && !seen.insert(mode.mode_id.clone()) {
        diagnostics.push(diagnostic(
            "domain_pack.duplicate_mode",
            format!("duplicate mode {}", mode.mode_id),
        ));
    }
}

fn validate_workflow_phase(
    phase: &WorkflowPhase,
    seen: &mut HashSet<String>,
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    require_non_empty(diagnostics, "workflow_phase.phase_id", &phase.phase_id);
    require_non_empty(diagnostics, "workflow_phase.label", &phase.label);
    require_non_empty(
        diagnostics,
        "workflow_phase.description",
        &phase.description,
    );
    if !phase.phase_id.is_empty() && !seen.insert(phase.phase_id.clone()) {
        diagnostics.push(diagnostic(
            "domain_pack.duplicate_workflow_phase",
            format!("duplicate workflow phase {}", phase.phase_id),
        ));
    }
}

fn validate_acp_persona(
    persona: &AcpPersonaDeclaration,
    seen: &mut HashSet<String>,
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    require_non_empty(diagnostics, "acp_persona.persona_id", &persona.persona_id);
    require_non_empty(diagnostics, "acp_persona.label", &persona.label);
    require_non_empty(diagnostics, "acp_persona.description", &persona.description);
    if !persona.persona_id.is_empty() && !seen.insert(persona.persona_id.clone()) {
        diagnostics.push(diagnostic(
            "domain_pack.duplicate_acp_persona",
            format!("duplicate ACP persona {}", persona.persona_id),
        ));
    }
}

fn validate_resource_uri_scheme(
    scheme: &ResourceUriScheme,
    seen: &mut HashSet<String>,
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    require_non_empty(diagnostics, "resource_uri_scheme.scheme", &scheme.scheme);
    require_non_empty(
        diagnostics,
        "resource_uri_scheme.resource_kind",
        &scheme.resource_kind,
    );
    require_non_empty(
        diagnostics,
        "resource_uri_scheme.description",
        &scheme.description,
    );
    if !scheme.scheme.is_empty() && !seen.insert(scheme.scheme.clone()) {
        diagnostics.push(diagnostic(
            "domain_pack.duplicate_resource_uri_scheme",
            format!("duplicate resource URI scheme {}", scheme.scheme),
        ));
    }
}

fn validate_external_mcp_transport(
    transport: &ExternalMcpTransport,
    probe_ids: &HashSet<&str>,
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    require_non_empty(diagnostics, "mcp.server_id", &transport.server_id);
    require_non_empty(diagnostics, "mcp.description", &transport.description);
    if !transport.read_only {
        diagnostics.push(diagnostic(
            "domain_pack.mcp_transport_not_read_only",
            format!("{} is not read-only", transport.server_id),
        ));
    }
    for probe_id in &transport.allowed_probe_ids {
        if !probe_ids.contains(probe_id.as_str()) {
            diagnostics.push(diagnostic(
                "domain_pack.mcp_transport_unknown_probe",
                format!(
                    "{} references unknown probe {}",
                    transport.server_id, probe_id
                ),
            ));
        }
    }
}

fn yaml_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("yaml") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn parse_yaml_file(path: &Path) -> Result<serde_yaml::Value> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read yaml {}", path.display()))?;
    serde_yaml::from_str(&source)
        .with_context(|| format!("failed to parse yaml {}", path.display()))
}

fn yaml_field<'a>(value: &'a serde_yaml::Value, key: &str) -> Option<&'a serde_yaml::Value> {
    value
        .as_mapping()?
        .get(serde_yaml::Value::String(key.to_string()))
}

fn find_yaml_by_field(
    dir: &Path,
    fields: &[&str],
    expected: &str,
) -> Result<(PathBuf, serde_yaml::Value)> {
    for path in yaml_files(dir)? {
        let yaml = parse_yaml_file(&path)?;
        if fields.iter().any(|field| {
            yaml_field(&yaml, field).and_then(serde_yaml::Value::as_str) == Some(expected)
        }) {
            return Ok((path, yaml));
        }
    }

    bail!(
        "failed to find yaml in {} with any of {:?} = {}",
        dir.display(),
        fields,
        expected
    )
}

fn owned_pack_macro_fqns(config_root: &Path, pack_id: &str) -> Result<Vec<String>> {
    let (_, pack_yaml) = find_yaml_by_field(&config_root.join("packs"), &["id"], pack_id)?;
    let macro_defs = macro_definition_index(&config_root.join("verb_schemas/macros"))?;
    Ok(yaml_field(&pack_yaml, "allowed_verbs")
        .and_then(serde_yaml::Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(serde_yaml::Value::as_str)
        .filter(|allowed| macro_defs.contains_key(*allowed))
        .map(ToString::to_string)
        .collect())
}

fn find_macro_yaml_by_fqn(dir: &Path, expected: &str) -> Result<(PathBuf, serde_yaml::Value)> {
    macro_definition_index(dir)?
        .remove(expected)
        .with_context(|| {
            format!(
                "failed to find macro definition {expected} in {}",
                dir.display()
            )
        })
}

fn macro_definition_index(dir: &Path) -> Result<BTreeMap<String, (PathBuf, serde_yaml::Value)>> {
    let mut out = BTreeMap::new();
    for path in yaml_files(dir)? {
        let yaml = parse_yaml_file(&path)?;
        let Some(mapping) = yaml.as_mapping() else {
            continue;
        };
        for (key, body) in mapping {
            let Some(fqn) = key.as_str() else {
                continue;
            };
            if out
                .insert(fqn.to_string(), (path.clone(), body.clone()))
                .is_some()
            {
                bail!("duplicate macro definition {fqn}");
            }
        }
    }
    Ok(out)
}

fn insert_surface(
    surfaces: &mut BTreeMap<String, DomainPackTaxonomySurface>,
    key: String,
    path: PathBuf,
    yaml: &serde_yaml::Value,
) -> Result<()> {
    let canonical_payload = canonical_yaml_json(yaml)?;
    surfaces.insert(
        key,
        DomainPackTaxonomySurface {
            path,
            canonical_payload,
        },
    );
    Ok(())
}

fn canonical_yaml_json(value: &serde_yaml::Value) -> Result<String> {
    fn sort_json(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(sort_json).collect())
            }
            serde_json::Value::Object(map) => {
                let mut sorted = serde_json::Map::new();
                let mut entries = map.into_iter().collect::<Vec<_>>();
                entries.sort_by(|left, right| left.0.cmp(&right.0));
                for (key, value) in entries {
                    sorted.insert(key, sort_json(value));
                }
                serde_json::Value::Object(sorted)
            }
            scalar => scalar,
        }
    }

    let json = serde_json::to_value(value).context("failed to convert yaml to json")?;
    serde_json::to_string(&sort_json(json)).context("failed to serialize canonical json")
}

fn validate_owned_entity_kinds(
    entity_taxonomy: &serde_yaml::Value,
    owned_entity_kinds: &[String],
) -> Result<()> {
    let Some(entity_defs) = yaml_field(entity_taxonomy, "entities").and_then(|v| v.as_mapping())
    else {
        bail!("entity taxonomy does not declare entities");
    };

    for entity_kind in owned_entity_kinds {
        if !entity_defs.contains_key(serde_yaml::Value::String(entity_kind.clone())) {
            bail!("domain pack owns unknown entity kind {entity_kind}");
        }
    }

    Ok(())
}

fn hash_surfaces(surfaces: &BTreeMap<String, DomainPackTaxonomySurface>) -> String {
    let mut hasher = Sha256::new();
    for (surface, payload) in surfaces {
        hasher.update(surface.as_bytes());
        hasher.update(b"\n");
        hasher.update(payload.canonical_payload.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

fn validate_owned_values(
    field: &'static str,
    values: &[String],
    diagnostics: &mut Vec<DomainPackDiagnostic>,
) {
    let mut seen = HashSet::new();
    for value in values {
        if value.trim().is_empty() {
            diagnostics.push(diagnostic(
                "domain_pack.owned_field_empty",
                format!("{field} contains an empty value"),
            ));
        } else if !seen.insert(value.as_str()) {
            diagnostics.push(diagnostic(
                "domain_pack.duplicate_owned_value",
                format!("{field} contains duplicate value {value}"),
            ));
        }
    }
}

fn require_non_empty(
    diagnostics: &mut Vec<DomainPackDiagnostic>,
    field: &'static str,
    value: &str,
) {
    if value.trim().is_empty() {
        diagnostics.push(diagnostic(
            "domain_pack.required_field_empty",
            format!("{field} must not be empty"),
        ));
    }
}

fn diagnostic(code: &'static str, message: impl Into<String>) -> DomainPackDiagnostic {
    DomainPackDiagnostic {
        code: code.to_string(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn valid_manifest() -> DomainPackManifest {
        DomainPackManifest {
            pack_id: "ob-poc.kyc".to_string(),
            name: "ob-poc KYC".to_string(),
            version: "0.1.0".to_string(),
            implementation_mode: PackImplementationMode::NativeCompiled,
            compatibility_tier: PackCompatibilityTier::DryRunOnly,
            owned_dags: vec!["kyc_dag".to_string()],
            owned_packs: vec!["kyc-case".to_string()],
            owned_state_machines: vec!["kyc_case_lifecycle".to_string()],
            owned_constellation_maps: vec!["kyc.onboarding".to_string()],
            owned_constellation_families: vec!["kyc_lifecycle".to_string()],
            owned_universes: vec!["universe.kyc_operations".to_string()],
            owned_verb_prefixes: vec!["kyc-case.".to_string()],
            owned_entity_kinds: vec!["kyc_case".to_string()],
            business_crates: vec![],
            owned_constellations: vec!["kyc.onboarding".to_string()],
            allowed_transitions: vec![DomainTransition {
                transition_ref: "kyc-case.intake-to-discovery".to_string(),
                entity_type: "kyc_case".to_string(),
                state_machine: "kyc_case_lifecycle".to_string(),
                verb: "kyc-case.update-status".to_string(),
                from_state: "INTAKE".to_string(),
                to_state: "DISCOVERY".to_string(),
                dry_run_enabled: true,
                mutation_enabled: false,
                hitl_required: true,
                evidence_refs_required: vec!["case_id".to_string()],
            }],
            discovery_probes: vec![DiscoveryProbe {
                probe_id: "kyc-case.read-state".to_string(),
                operation: "read_state".to_string(),
                target: "\"ob-poc\".cases.status".to_string(),
                idempotent: true,
                modeled: true,
                first_class_state_mutation: false,
            }],
            projection_catalog: vec![ProjectionCatalogEntry {
                kind: AcpProjectionKind::PackManifest,
                source: "domain_pack.manifest".to_string(),
                default_classification: ClassificationLimit::Internal,
                allowed_subject_kinds: vec![],
                max_depth: None,
                acp_visible_by_default: true,
            }],
            mention_namespaces: vec![MentionNamespace {
                namespace: "entity".to_string(),
                target_kind: "semantic_entity".to_string(),
                description: "SemOS entity reference".to_string(),
            }],
            declared_modes: vec![DeclaredMode {
                mode_id: "discovery".to_string(),
                label: "Discovery".to_string(),
                description: "Read-only substrate exploration".to_string(),
            }],
            workflow_phases: vec![WorkflowPhase {
                phase_id: "discovery".to_string(),
                label: "Discovery".to_string(),
                description: "Read-only substrate exploration".to_string(),
            }],
            acp_personas: vec![
                AcpPersonaDeclaration {
                    persona_id: "sage:planning".to_string(),
                    label: "Sage Planning".to_string(),
                    description: "Discovery, projection, planning, and workbook drafting"
                        .to_string(),
                    mutation_authority: false,
                },
                AcpPersonaDeclaration {
                    persona_id: "sage:execution".to_string(),
                    label: "Sage Execution".to_string(),
                    description: "Workbook validation, dry-run, and approved execution".to_string(),
                    mutation_authority: true,
                },
            ],
            resource_uri_schemes: vec![ResourceUriScheme {
                scheme: "semos://entity/{id}".to_string(),
                resource_kind: "entity".to_string(),
                description: "SemOS entity reference".to_string(),
            }],
            external_mcp_transports: vec![],
            typed_extension_points: vec![TypedExtensionPoint {
                extension_id: "derivation.registry".to_string(),
                extension_kind: "derivation_registry".to_string(),
                implementation_ref: "sem_os_core::derivation::DerivationFunctionRegistry"
                    .to_string(),
            }],
            classification_policy: ContextClassificationPolicy {
                max_prompt_classification: ClassificationLimit::Internal,
                allow_external_llm: false,
                required_redactions: vec!["pii".to_string()],
            },
        }
    }

    #[test]
    fn valid_manifest_passes() {
        let report = valid_manifest().validate();
        assert!(report.valid, "{:?}", report.diagnostics);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn mutation_requires_hitl_and_dry_run() {
        let mut manifest = valid_manifest();
        let transition = &mut manifest.allowed_transitions[0];
        transition.dry_run_enabled = false;
        transition.mutation_enabled = true;
        transition.hitl_required = false;

        let report = manifest.validate();

        assert!(!report.valid);
        let codes: Vec<_> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"domain_pack.mutation_requires_hitl"));
        assert!(codes.contains(&"domain_pack.mutation_requires_dry_run"));
        assert!(codes.contains(&"domain_pack.mutation_not_allowed_for_tier"));
    }

    #[test]
    fn probes_must_be_safe_and_modeled() {
        let mut manifest = valid_manifest();
        let probe = &mut manifest.discovery_probes[0];
        probe.idempotent = false;
        probe.modeled = false;
        probe.first_class_state_mutation = true;

        let report = manifest.validate();

        assert!(!report.valid);
        let codes: Vec<_> = report.diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert!(codes.contains(&"domain_pack.probe_not_idempotent"));
        assert!(codes.contains(&"domain_pack.probe_not_modeled"));
        assert!(codes.contains(&"domain_pack.probe_mutates_state"));
    }

    #[test]
    fn duplicate_transition_refs_are_rejected() {
        let mut manifest = valid_manifest();
        manifest
            .allowed_transitions
            .push(manifest.allowed_transitions[0].clone());

        let report = manifest.validate();

        assert!(!report.valid);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "domain_pack.duplicate_transition_ref"));
    }

    #[test]
    fn duplicate_projection_kinds_are_rejected() {
        let mut manifest = valid_manifest();
        manifest
            .projection_catalog
            .push(manifest.projection_catalog[0].clone());

        let report = manifest.validate();

        assert!(!report.valid);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "domain_pack.duplicate_projection_kind"));
    }

    #[test]
    fn duplicate_owned_values_are_rejected() {
        let mut manifest = valid_manifest();
        manifest.owned_packs.push("kyc-case".to_string());

        let report = manifest.validate();

        assert!(!report.valid);
        assert!(report
            .diagnostics
            .iter()
            .any(|d| d.code == "domain_pack.duplicate_owned_value"));
    }

    #[test]
    fn discovery_probe_authorization_allows_declared_safe_probe() {
        let manifest = valid_manifest();
        let request = DiscoveryRequest {
            pack_id: "ob-poc.kyc".to_string(),
            probe_id: "kyc-case.read-state".to_string(),
            subject: DiscoverySubject {
                subject_kind: "kyc_case".to_string(),
                subject_id: "case-1".to_string(),
            },
            context: BTreeMap::new(),
        };

        let probe = authorize_discovery_probe(&manifest, &request).expect("probe authorized");

        assert_eq!(probe.operation, "read_state");
    }

    #[test]
    fn discovery_probe_authorization_refuses_unknown_probe() {
        let manifest = valid_manifest();
        let request = DiscoveryRequest {
            pack_id: "ob-poc.kyc".to_string(),
            probe_id: "kyc-case.write-state".to_string(),
            subject: DiscoverySubject {
                subject_kind: "kyc_case".to_string(),
                subject_id: "case-1".to_string(),
            },
            context: BTreeMap::new(),
        };

        let err = authorize_discovery_probe(&manifest, &request).expect_err("probe refused");

        assert_eq!(
            err,
            DiscoveryAuthorizationError::UnknownProbe {
                probe_id: "kyc-case.write-state".to_string()
            }
        );
    }

    #[test]
    fn ob_poc_kyc_seed_pack_parses_and_validates() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/sem_os_seeds/domain_packs/ob_poc_kyc.yaml");
        let contents = fs::read_to_string(&path).expect("domain pack readable");
        let manifest: DomainPackManifest =
            serde_yaml::from_str(&contents).expect("domain pack parses");

        let report = manifest.validate();

        assert!(report.valid, "{:?}", report.diagnostics);
        assert_eq!(manifest.pack_id, "ob-poc.kyc");
        assert!(manifest
            .allowed_transitions
            .iter()
            .any(|t| t.verb == "kyc-case.update-status"));
    }

    #[test]
    fn ob_poc_cbu_seed_pack_parses_and_validates() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/sem_os_seeds/domain_packs/ob_poc_cbu.yaml");
        let contents = fs::read_to_string(&path).expect("domain pack readable");
        let manifest: DomainPackManifest =
            serde_yaml::from_str(&contents).expect("domain pack parses");

        let report = manifest.validate();

        assert!(report.valid, "{:?}", report.diagnostics);
        assert_eq!(manifest.pack_id, "ob-poc.cbu");
        assert!(manifest.owned_dags.iter().any(|dag| dag == "cbu_dag"));
        assert!(manifest
            .owned_verb_prefixes
            .iter()
            .any(|prefix| prefix == "cbu."));
    }

    #[test]
    fn cbu_taxonomy_reload_from_yaml_is_idempotent() {
        let config_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config");

        let first =
            reload_domain_pack_taxonomy_from_yaml(&config_root, "ob-poc.cbu").expect("first load");
        let second =
            reload_domain_pack_taxonomy_from_yaml(&config_root, "ob-poc.cbu").expect("second load");

        assert_eq!(first, second);
        assert_eq!(first.manifest.pack_id, "ob-poc.cbu");
        assert!(first.manifest.owned_dags.iter().any(|dag| dag == "cbu_dag"));
        assert!(first.surfaces.contains_key("dag:cbu_dag"));
        assert!(first.surfaces.contains_key("pack:cbu-maintenance"));
        assert!(!first.surface_hash.is_empty());
    }

    #[test]
    fn reload_index_skips_when_source_fingerprints_match() {
        let config_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config");
        let now = Utc::now();
        let reload =
            reload_domain_pack_taxonomy_from_yaml(&config_root, "ob-poc.cbu").expect("load");
        let index = reload_index_entry_from_reload(
            &config_root,
            &reload,
            now,
            DomainPackReloadStatus::Loaded,
        )
        .expect("index");

        let plan = refresh_domain_pack_taxonomy_with_index(
            &config_root,
            "ob-poc.cbu",
            Some(&index),
            false,
            now,
        )
        .expect("plan");

        assert_eq!(plan.action, DomainPackRefreshAction::Skip);
        assert_eq!(plan.index_entry.surface_hash, reload.surface_hash);
        assert!(plan.reload.is_none());
    }

    #[test]
    fn reload_index_updates_only_when_fingerprint_changed_but_hash_matches() {
        let config_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config");
        let now = Utc::now();
        let reload =
            reload_domain_pack_taxonomy_from_yaml(&config_root, "ob-poc.cbu").expect("load");
        let mut index = reload_index_entry_from_reload(
            &config_root,
            &reload,
            now,
            DomainPackReloadStatus::Loaded,
        )
        .expect("index");
        index.source_fingerprints[0].size_bytes += 1;

        let plan = refresh_domain_pack_taxonomy_with_index(
            &config_root,
            "ob-poc.cbu",
            Some(&index),
            false,
            now,
        )
        .expect("plan");

        assert_eq!(plan.action, DomainPackRefreshAction::IndexOnly);
        assert_eq!(plan.index_entry.surface_hash, reload.surface_hash);
        assert!(plan.reload.is_some());
    }

    #[test]
    fn reload_index_requires_publish_without_prior_index() {
        let config_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config");

        let plan = refresh_domain_pack_taxonomy_with_index(
            &config_root,
            "ob-poc.cbu",
            None,
            false,
            Utc::now(),
        )
        .expect("plan");

        assert_eq!(plan.action, DomainPackRefreshAction::PublishRequired);
        assert_eq!(
            plan.index_entry.status,
            DomainPackReloadStatus::PublishRequired
        );
        assert!(plan.reload.is_some());
    }

    #[test]
    fn all_domain_packs_reload_idempotently_and_cover_dsl_surfaces() {
        let config_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config");

        let mut owned_packs = HashSet::new();
        let mut owned_dags = HashSet::new();
        let mut loaded_pack_ids = Vec::new();

        let all_reloads = reload_all_domain_pack_taxonomies_from_yaml(&config_root)
            .expect("all domain packs reload");

        for (pack_id, first) in &all_reloads {
            let second = reload_domain_pack_taxonomy_from_yaml(&config_root, pack_id)
                .expect("domain pack reload succeeds again");

            assert_eq!(*first, second, "reload was not idempotent for {pack_id}");
            loaded_pack_ids.push(pack_id.to_string());
            owned_packs.extend(first.manifest.owned_packs.iter().cloned());
            owned_dags.extend(first.manifest.owned_dags.iter().cloned());
        }

        assert!(
            !loaded_pack_ids.is_empty(),
            "expected at least one SemOS domain pack"
        );

        let actual_packs = yaml_files(&config_root.join("packs"))
            .expect("pack directory readable")
            .into_iter()
            .map(|path| {
                let yaml = parse_yaml_file(&path).expect("pack yaml parses");
                yaml_field(&yaml, "id")
                    .and_then(serde_yaml::Value::as_str)
                    .unwrap_or_else(|| panic!("pack {} declares id", path.display()))
                    .to_string()
            })
            .collect::<HashSet<_>>();

        let actual_dags = yaml_files(&config_root.join("sem_os_seeds/dag_taxonomies"))
            .expect("DAG directory readable")
            .into_iter()
            .map(|path| {
                let yaml = parse_yaml_file(&path).expect("DAG yaml parses");
                yaml_field(&yaml, "dag_id")
                    .and_then(serde_yaml::Value::as_str)
                    .unwrap_or_else(|| panic!("DAG {} declares dag_id", path.display()))
                    .to_string()
            })
            .collect::<HashSet<_>>();

        let missing_packs = actual_packs
            .difference(&owned_packs)
            .cloned()
            .collect::<Vec<_>>();
        let missing_dags = actual_dags
            .difference(&owned_dags)
            .cloned()
            .collect::<Vec<_>>();

        assert!(
            missing_packs.is_empty(),
            "DSL journey packs not owned by any SemOS domain pack: {missing_packs:#?}"
        );
        assert!(
            missing_dags.is_empty(),
            "DAG taxonomies not owned by any SemOS domain pack: {missing_dags:#?}"
        );
    }
}
