//! Mod-defined parameter extension fields.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Mod-defined parameter fields keyed by namespaced field name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParameterExtensions {
    pub fields: BTreeMap<String, ParameterValue>,
}

/// A single extension parameter value from a mod.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParameterValue {
    pub source_mod: String,
    pub field_name: String,
    pub value: ParameterValueData,
}

/// Typed extension value payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ParameterValueData {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum { type_id: String, variant: String },
}
