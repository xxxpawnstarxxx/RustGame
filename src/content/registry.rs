//! The [`ContentRegistry`]: loading, validation and lookup of every content
//! definition category.
//!
//! # Loading
//!
//! `data/manifest.json` describes the content package (id, version, format
//! version). Each category lives in `data/<category>/*.json`; each file is a
//! JSON array of definitions. Files are read in file-name order so loading
//! is deterministic, and every byte read contributes to the content
//! *fingerprint* recorded in saves.
//!
//! # Validation
//!
//! Validation never stops at the first problem: every parse error, duplicate
//! id, dangling cross-reference and range violation is collected and reported
//! together as [`SimError::ContentValidation`], each message prefixed with the
//! file it came from.
//!
//! # Adding a category
//!
//! Define the struct, implement [`Definition`] (category directory name, id,
//! references, intrinsic checks) and add one line to the `content_registry!`
//! invocation at the bottom of this file.

use super::defs::*;
use super::ids::DefKey;
use crate::core::config::{GameConfig, PeriodConfig};
use crate::core::error::{SimError, SimResult};
use crate::core::hash::Fnv64;
use crate::core::time::Calendar;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// Content package format understood by this build.
pub const CONTENT_FORMAT_VERSION: u32 = 1;

/// `data/manifest.json`: identifies and versions a content package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentManifest {
    pub id: String,
    pub name: String,
    /// Semantic version of the content itself.
    pub version: String,
    /// Layout/schema version of the package format.
    pub format_version: u32,
    #[serde(default)]
    pub description: String,
}

impl Default for ContentManifest {
    fn default() -> Self {
        Self {
            id: "unversioned".into(),
            name: "Unversioned content".into(),
            version: "0.0.0".into(),
            format_version: CONTENT_FORMAT_VERSION,
            description: String::new(),
        }
    }
}

/// Compact identity of the loaded content, recorded in save files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentStamp {
    pub package: String,
    pub version: String,
    /// Hex FNV-1a fingerprint of every content file.
    pub fingerprint: String,
}

/// A cross-reference from one definition to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub field: &'static str,
    pub category: &'static str,
    pub id: String,
}

impl Reference {
    pub fn new(field: &'static str, category: &'static str, id: &impl DefKey) -> Self {
        Self {
            field,
            category,
            id: id.as_str().to_owned(),
        }
    }
}

/// Everything a definition may consult while validating itself.
pub struct ValidationEnv<'a> {
    pub calendar: &'a Calendar,
    pub period: &'a PeriodConfig,
    pub content: &'a ContentRegistry,
}

impl ValidationEnv<'_> {
    /// Checks an optional technology requirement against the period.
    pub fn check_technology(
        &self,
        technology: &Option<super::TechnologyId>,
        errors: &mut Vec<String>,
    ) {
        if let Some(tech) = technology
            && !self.period.has_technology(tech.as_str())
        {
            errors.push(format!(
                "requires technology '{tech}', which is not available in period '{}'",
                self.period.name
            ));
        }
    }
}

/// A content definition type loadable from `data/<CATEGORY>/*.json`.
pub trait Definition: DeserializeOwned + Clone + fmt::Debug {
    type Id: DefKey;
    /// Directory name under the data root; also names the category in errors.
    const CATEGORY: &'static str;

    fn id(&self) -> &Self::Id;

    /// Ids of other definitions this one refers to. Checked generically.
    fn references(&self) -> Vec<Reference> {
        Vec::new()
    }

    /// Intrinsic and contextual checks; returns problem descriptions.
    fn validate(&self, _env: &ValidationEnv<'_>) -> Vec<String> {
        Vec::new()
    }
}

/// Definition types stored in the [`ContentRegistry`].
pub trait Registered: Definition {
    fn table(registry: &ContentRegistry) -> &DefTable<Self>;
    fn table_mut(registry: &mut ContentRegistry) -> &mut DefTable<Self>;
}

/// One reported content problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentIssue {
    pub origin: String,
    pub category: &'static str,
    pub id: String,
    pub message: String,
}

impl fmt::Display for ContentIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.id.is_empty() {
            write!(f, "{} [{}]: {}", self.origin, self.category, self.message)
        } else {
            write!(
                f,
                "{} [{} '{}']: {}",
                self.origin, self.category, self.id, self.message
            )
        }
    }
}

/// Table of definitions of one type, ordered by id.
#[derive(Debug, Clone)]
pub struct DefTable<T: Definition> {
    items: BTreeMap<T::Id, T>,
    origins: BTreeMap<T::Id, String>,
}

impl<T: Definition> Default for DefTable<T> {
    fn default() -> Self {
        Self {
            items: BTreeMap::new(),
            origins: BTreeMap::new(),
        }
    }
}

impl<T: Definition> DefTable<T> {
    /// Adds a definition. A duplicate id is reported naming both origins.
    pub fn insert(&mut self, def: T, origin: &str) -> Result<(), ContentIssue> {
        let id = def.id().clone();
        if let Some(first) = self.origins.get(id.as_str()) {
            return Err(ContentIssue {
                origin: origin.to_owned(),
                category: T::CATEGORY,
                id: id.to_string(),
                message: format!("duplicate id (first defined in {first})"),
            });
        }
        self.origins.insert(id.clone(), origin.to_owned());
        self.items.insert(id, def);
        Ok(())
    }

    pub fn get<Q>(&self, id: &Q) -> Option<&T>
    where
        T::Id: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.items.get(id)
    }

    /// Looks up a definition, producing a typed error when it is missing.
    pub fn require<Q>(&self, id: &Q) -> SimResult<&T>
    where
        T::Id: Borrow<Q>,
        Q: Ord + fmt::Display + ?Sized,
    {
        self.get(id).ok_or_else(|| SimError::UnknownDefinition {
            category: T::CATEGORY,
            id: id.to_string(),
        })
    }

    pub fn contains<Q>(&self, id: &Q) -> bool
    where
        T::Id: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.items.contains_key(id)
    }

    /// File a definition was loaded from.
    pub fn origin<Q>(&self, id: &Q) -> &str
    where
        T::Id: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.origins.get(id).map_or("<memory>", String::as_str)
    }

    /// Position of an id in sorted order (stable palette index).
    pub fn index_of<Q>(&self, id: &Q) -> Option<usize>
    where
        T::Id: Borrow<Q>,
        Q: Ord + ?Sized,
    {
        self.items.keys().position(|k| k.borrow() == id)
    }

    /// Definition at a palette index.
    pub fn at(&self, index: usize) -> Option<&T> {
        self.items.values().nth(index)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Definitions in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.values()
    }

    pub fn ids(&self) -> impl Iterator<Item = &T::Id> {
        self.items.keys()
    }
}

fn load_category<T: Registered>(
    registry: &mut ContentRegistry,
    data_dir: &Path,
    hasher: &mut Fnv64,
    issues: &mut Vec<ContentIssue>,
) -> SimResult<()> {
    let dir = data_dir.join(T::CATEGORY);
    if !dir.is_dir() {
        registry.warnings.push(format!(
            "category '{}' has no directory; it is empty",
            T::CATEGORY
        ));
        return Ok(());
    }
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| SimError::Io { path, source }
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(io(&dir))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    for path in files {
        let bytes = std::fs::read(&path).map_err(io(&path))?;
        let relative = path
            .strip_prefix(data_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        hasher.write_str(&relative);
        hasher.write(&bytes);
        let origin = format!("data/{relative}");
        match serde_json::from_slice::<Vec<T>>(&bytes) {
            Ok(defs) => {
                for def in defs {
                    if let Err(issue) = T::table_mut(registry).insert(def, &origin) {
                        issues.push(issue);
                    }
                }
            }
            Err(err) => issues.push(ContentIssue {
                origin,
                category: T::CATEGORY,
                id: String::new(),
                message: format!("parse error: {err}"),
            }),
        }
    }
    Ok(())
}

fn validate_table<T: Registered>(
    registry: &ContentRegistry,
    env: &ValidationEnv<'_>,
    issues: &mut Vec<ContentIssue>,
) {
    let table = T::table(registry);
    for def in table.iter() {
        let id = def.id();
        let issue = |message: String| ContentIssue {
            origin: table.origin(id.as_str()).to_owned(),
            category: T::CATEGORY,
            id: id.to_string(),
            message,
        };
        for r in def.references() {
            if !registry.has(r.category, &r.id) {
                issues.push(issue(format!(
                    "{} refers to unknown {} '{}'",
                    r.field, r.category, r.id
                )));
            }
        }
        for message in def.validate(env) {
            issues.push(issue(message));
        }
    }
}

macro_rules! content_registry {
    ($( $field:ident : $ty:ty ),* $(,)?) => {
        /// All immutable content definitions, shared read-only by every system.
        #[derive(Debug, Clone, Default)]
        pub struct ContentRegistry {
            manifest: ContentManifest,
            fingerprint: u64,
            warnings: Vec<String>,
            $( $field: DefTable<$ty>, )*
        }

        $(
            impl Registered for $ty {
                fn table(registry: &ContentRegistry) -> &DefTable<Self> {
                    &registry.$field
                }
                fn table_mut(registry: &mut ContentRegistry) -> &mut DefTable<Self> {
                    &mut registry.$field
                }
            }
        )*

        impl ContentRegistry {
            $(
                pub fn $field(&self) -> &DefTable<$ty> {
                    &self.$field
                }
            )*

            fn load_categories(&mut self, dir: &Path, hasher: &mut Fnv64, issues: &mut Vec<ContentIssue>) -> SimResult<()> {
                $( load_category::<$ty>(self, dir, hasher, issues)?; )*
                Ok(())
            }

            fn validate_definitions(&self, env: &ValidationEnv<'_>, issues: &mut Vec<ContentIssue>) {
                $( validate_table::<$ty>(self, env, issues); )*
            }

            /// Whether a definition with this id exists in the named category.
            pub fn has(&self, category: &str, id: &str) -> bool {
                $(
                    if category == <$ty as Definition>::CATEGORY {
                        return self.$field.contains(id);
                    }
                )*
                false
            }

            /// Number of definitions per category, in registry order.
            pub fn category_counts(&self) -> Vec<(&'static str, usize)> {
                vec![ $( (<$ty as Definition>::CATEGORY, self.$field.len()), )* ]
            }
        }
    };
}

content_registry! {
    activities: ActivityDef,
    routines: DailyRoutineDef,
    technologies: TechnologyDef,
    materials: MaterialDef,
    items: ItemDef,
    weapons: WeaponDef,
    armour: ArmourDef,
    foods: FoodDef,
    resources: ResourceDef,
    recipes: RecipeDef,
    body_templates: BodyTemplateDef,
    injuries: InjuryDef,
    skills: SkillDef,
    professions: ProfessionDef,
    jobs: JobDef,
    characters: CharacterTemplateDef,
    buildings: BuildingDef,
    animals: AnimalDef,
    crops: CropDef,
    trees: TreeSpeciesDef,
    laws: LawDef,
    factions: FactionDef,
    worldgen: WorldGenPresetDef,
}

impl ContentRegistry {
    /// A registry with no definitions (tests and tools insert their own).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Loads the package manifest and every category under `data_dir`, then
    /// validates everything against the configuration.
    pub fn load(data_dir: &Path, config: &GameConfig) -> SimResult<Self> {
        let mut registry = Self::empty();
        let mut hasher = Fnv64::new();
        let mut issues = Vec::new();

        let manifest_path = data_dir.join("manifest.json");
        match std::fs::read(&manifest_path) {
            Ok(bytes) => {
                hasher.write(&bytes);
                match serde_json::from_slice::<ContentManifest>(&bytes) {
                    Ok(manifest) => registry.manifest = manifest,
                    Err(err) => issues.push(ContentIssue {
                        origin: "data/manifest.json".into(),
                        category: "manifest",
                        id: String::new(),
                        message: format!("parse error: {err}"),
                    }),
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => registry
                .warnings
                .push("data/manifest.json is missing; content is unversioned".into()),
            Err(source) => {
                return Err(SimError::Io {
                    path: manifest_path,
                    source,
                });
            }
        }
        if registry.manifest.format_version != CONTENT_FORMAT_VERSION {
            return Err(SimError::content(
                "data/manifest.json",
                format!(
                    "content format version {} is not supported (this build reads version {CONTENT_FORMAT_VERSION})",
                    registry.manifest.format_version
                ),
            ));
        }

        registry.load_categories(data_dir, &mut hasher, &mut issues)?;
        registry.fingerprint = hasher.finish();
        registry.collect_issues(config, &mut issues)?;
        if !issues.is_empty() {
            return Err(SimError::ContentValidation(
                issues.iter().map(ToString::to_string).collect(),
            ));
        }
        for warning in &registry.warnings {
            tracing::warn!("{warning}");
        }
        tracing::info!(
            package = %registry.manifest.id,
            version = %registry.manifest.version,
            definitions = registry.category_counts().iter().map(|(_, n)| n).sum::<usize>(),
            "content loaded"
        );
        Ok(registry)
    }

    /// Adds one definition (in-memory construction).
    pub fn insert<T: Registered>(&mut self, def: T) -> SimResult<()> {
        T::table_mut(self)
            .insert(def, "<memory>")
            .map_err(|issue| SimError::ContentValidation(vec![issue.to_string()]))
    }

    /// Adds definitions, builder style.
    pub fn with<T: Registered>(mut self, defs: impl IntoIterator<Item = T>) -> SimResult<Self> {
        for def in defs {
            self.insert(def)?;
        }
        Ok(self)
    }

    /// Validates an in-memory registry against a configuration.
    pub fn validate(&self, config: &GameConfig) -> SimResult<()> {
        let mut issues = Vec::new();
        self.collect_issues(config, &mut issues)?;
        if issues.is_empty() {
            Ok(())
        } else {
            Err(SimError::ContentValidation(
                issues.iter().map(ToString::to_string).collect(),
            ))
        }
    }

    fn collect_issues(&self, config: &GameConfig, issues: &mut Vec<ContentIssue>) -> SimResult<()> {
        let calendar = Calendar::new(&config.calendar)?;
        let env = ValidationEnv {
            calendar: &calendar,
            period: &config.period,
            content: self,
        };
        self.validate_definitions(&env, issues);
        self.validate_global(config, issues);
        Ok(())
    }

    /// Checks spanning several definitions or the configuration.
    fn validate_global(&self, config: &GameConfig, issues: &mut Vec<ContentIssue>) {
        let global = |category: &'static str, message: String| ContentIssue {
            origin: "content".into(),
            category,
            id: String::new(),
            message,
        };
        if !self.technologies.is_empty() {
            for tech in &config.period.technologies {
                if !self.technologies.contains(tech.as_str()) {
                    issues.push(global(
                        "technologies",
                        format!("period technology '{tech}' has no technology definition"),
                    ));
                }
            }
            if let Some(cycle) = technology_cycle(&self.technologies) {
                issues.push(global(
                    "technologies",
                    format!("prerequisite cycle involving '{cycle}'"),
                ));
            }
        }
        if !self.worldgen.is_empty()
            && !self
                .worldgen
                .contains(config.simulation.default_world_preset.as_str())
        {
            issues.push(global(
                "worldgen",
                format!(
                    "configured default_world_preset '{}' does not exist",
                    config.simulation.default_world_preset
                ),
            ));
        }
        let movement = &config.movement;
        for (field, injury) in [
            ("movement.fall_minor_injury", &movement.fall_minor_injury),
            ("movement.fall_severe_injury", &movement.fall_severe_injury),
            ("movement.stumble_injury", &movement.stumble_injury),
        ] {
            if !self.injuries.is_empty() && !self.injuries.contains(injury.as_str()) {
                issues.push(global(
                    "injuries",
                    format!("{field} refers to unknown injury '{injury}'"),
                ));
            }
        }
    }

    pub fn manifest(&self) -> &ContentManifest {
        &self.manifest
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// Identity recorded in saves for compatibility checks.
    pub fn stamp(&self) -> ContentStamp {
        ContentStamp {
            package: self.manifest.id.clone(),
            version: self.manifest.version.clone(),
            fingerprint: crate::core::hash::hex64(self.fingerprint),
        }
    }

    /// Non-fatal observations made while loading.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Generic lookup by definition type.
    pub fn get<T: Registered>(&self, id: &str) -> Option<&T>
    where
        T::Id: Borrow<str>,
    {
        T::table(self).get(id)
    }
}

/// Returns a technology on a prerequisite cycle, if any.
fn technology_cycle(table: &DefTable<TechnologyDef>) -> Option<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Visiting,
        Done,
    }
    fn visit<'a>(
        id: &'a str,
        table: &'a DefTable<TechnologyDef>,
        marks: &mut BTreeMap<&'a str, Mark>,
    ) -> Option<String> {
        match marks.get(id) {
            Some(Mark::Done) => return None,
            Some(Mark::Visiting) => return Some(id.to_owned()),
            None => {}
        }
        marks.insert(id, Mark::Visiting);
        if let Some(def) = table.get(id) {
            for pre in &def.prerequisites {
                if let Some(cycle) = visit(pre.as_str(), table, marks) {
                    return Some(cycle);
                }
            }
        }
        marks.insert(id, Mark::Done);
        None
    }
    let mut marks = BTreeMap::new();
    let ids: BTreeSet<&str> = table.ids().map(|id| id.as_str()).collect();
    ids.into_iter().find_map(|id| visit(id, table, &mut marks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{ActivityCategory, ActivityId, RoutineBlockDef, RoutineId, TechnologyId};
    use crate::core::time::TimeOfDay;

    fn activity(id: &str) -> ActivityDef {
        ActivityDef {
            id: ActivityId::new(id),
            name: id.into(),
            category: ActivityCategory::Work,
            description: String::new(),
        }
    }

    fn routine(blocks: &[(u32, &str)]) -> DailyRoutineDef {
        DailyRoutineDef {
            id: RoutineId::new("r"),
            name: "r".into(),
            description: String::new(),
            blocks: blocks
                .iter()
                .map(|(h, a)| RoutineBlockDef {
                    start: TimeOfDay::hm(*h, 0),
                    activity: ActivityId::new(*a),
                })
                .collect(),
            weekday_overrides: BTreeMap::new(),
        }
    }

    fn tech(id: &str, pre: &[&str]) -> TechnologyDef {
        TechnologyDef {
            id: TechnologyId::new(id),
            name: id.into(),
            description: String::new(),
            prerequisites: pre.iter().map(|p| TechnologyId::new(*p)).collect(),
            introduced_year: None,
        }
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let result = ContentRegistry::empty().with(vec![activity("sleep"), activity("sleep")]);
        assert!(matches!(result, Err(SimError::ContentValidation(_))));
    }

    #[test]
    fn dangling_references_are_all_reported() {
        let config = GameConfig::default();
        let registry = ContentRegistry::empty()
            .with(vec![activity("sleep")])
            .unwrap()
            .with(vec![routine(&[(0, "sleep"), (6, "work"), (9, "pray")])])
            .unwrap();
        let Err(SimError::ContentValidation(problems)) = registry.validate(&config) else {
            panic!("expected validation failure");
        };
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("'work'")));
        assert!(problems.iter().any(|p| p.contains("'pray'")));
    }

    #[test]
    fn unordered_routine_blocks_are_rejected() {
        let registry = ContentRegistry::empty()
            .with(vec![activity("sleep"), activity("work")])
            .unwrap()
            .with(vec![routine(&[(6, "work"), (2, "sleep")])])
            .unwrap();
        assert!(registry.validate(&GameConfig::default()).is_err());
    }

    #[test]
    fn technology_cycles_and_period_gaps_are_detected() {
        let mut config = GameConfig::default();
        config.period.technologies = vec!["a".into(), "b".into(), "missing".into()];
        let registry = ContentRegistry::empty()
            .with(vec![tech("a", &["b"]), tech("b", &["a"])])
            .unwrap();
        let Err(SimError::ContentValidation(problems)) = registry.validate(&config) else {
            panic!("expected validation failure");
        };
        assert!(problems.iter().any(|p| p.contains("cycle")));
        assert!(problems.iter().any(|p| p.contains("'missing'")));
    }

    #[test]
    fn parse_errors_name_the_file_and_loading_continues() {
        let dir = tempfile::tempdir().unwrap();
        let acts = dir.path().join("activities");
        std::fs::create_dir_all(&acts).unwrap();
        std::fs::write(
            acts.join("a.json"),
            r#"[{"id":"sleep","name":"Sleep","category":"Rest"}]"#,
        )
        .unwrap();
        std::fs::write(acts.join("b.json"), r#"[{"id":"oops""#).unwrap();
        let Err(SimError::ContentValidation(problems)) =
            ContentRegistry::load(dir.path(), &GameConfig::default())
        else {
            panic!("expected validation failure");
        };
        assert!(
            problems
                .iter()
                .any(|p| p.contains("data/activities/b.json"))
        );
    }

    #[test]
    fn fingerprint_tracks_file_contents() {
        let dir = tempfile::tempdir().unwrap();
        let acts = dir.path().join("activities");
        std::fs::create_dir_all(&acts).unwrap();
        let file = acts.join("a.json");
        std::fs::write(
            &file,
            r#"[{"id":"sleep","name":"Sleep","category":"Rest"}]"#,
        )
        .unwrap();
        let config = GameConfig::default();
        let first = ContentRegistry::load(dir.path(), &config).unwrap();
        assert_eq!(first.manifest().id, "unversioned");
        std::fs::write(
            &file,
            r#"[{"id":"sleep","name":"Slumber","category":"Rest"}]"#,
        )
        .unwrap();
        let second = ContentRegistry::load(dir.path(), &config).unwrap();
        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn unsupported_format_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("manifest.json"),
            r#"{"id":"x","name":"X","version":"1.0.0","format_version":99}"#,
        )
        .unwrap();
        assert!(ContentRegistry::load(dir.path(), &GameConfig::default()).is_err());
    }
}
