//! # connector — Manifest-Connector framework (C1)
//!
//! Deklarative JSON-Manifeste binden externe Plattformen/Dienste an das
//! Overlay — kein fremder Code (docs/research/connector-system.md).
//! SCLM-spezifisches lebt ausschließlich im Manifest; dieser Core ist generisch.
//!
//! - manifest: serde-Schema (inkl. Formular-View + Header-Auth)
//! - validate: strenger Validator (https-only, keine file://-Pfade,
//!   Größenlimit) mit „Warnung ignorieren“-Option für Sebos „Verantwortung
//!   des Nutzers“-Beschluss
//! - mapping: Dot-Path-Engine (`"name": "data[].ship_name"`)

pub mod flow;
pub mod manifest;
pub mod mapping;
pub mod validate;

pub use flow::{AutocompleteRule, CondOp, Condition, Flow, Screen, ScreenType};
pub use manifest::{AuthType, ConnectorManifest, ScanSpec, View, ViewType};
pub use mapping::{DotPathError, dot_path};
pub use validate::{MAX_MANIFEST_BYTES, Report, validate_manifest};
