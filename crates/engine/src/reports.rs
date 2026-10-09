//! The JSON objects the result verbs print with `--json` (spec §8, DECISIONS Note J): one `dds_info` per DDS
//! for `sdc inspect` and one `dds_validation` per file for `sdc validate`. Their schemas are published in
//! `schema/` beside the event and manifest schemas (see [`crate::schemas`]).
use crate::codes::Warning;
use crate::config::{AlphaPolicy, Subtype};
use crate::dds::HeaderKind;
use crate::validate::alpha::AlphaStats;
use crate::validate::ValidationSummary;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The `schema` written into every `dds_info` and `dds_validation` object.
pub const REPORT_SCHEMA: u32 = 1;

/// The `kind` of a `dds_info` object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DdsInfoKind {
    DdsInfo,
}

/// The `kind` of a `dds_validation` object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DdsValidationKind {
    DdsValidation,
}

/// One mip level of a [`DdsInfo`]: its size and, for formats that carry alpha, the decoded-alpha statistics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DdsMipInfo {
    pub width: u32,
    pub height: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha: Option<AlphaStats>,
    /// Why the level could not be decoded for the statistics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decode_error: Option<String>,
}

/// `sdc inspect` on a DDS: the file's header facts and per-mip decoded-alpha statistics, or why it cannot be
/// read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DdsInfo {
    Info(DdsHeaderInfo),
    Unreadable(DdsUnreadable),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DdsHeaderInfo {
    pub kind: DdsInfoKind,
    pub schema: u32,
    /// '/'-separated, as given on the command line.
    pub path: String,
    /// The pixel format's name, e.g. `Bc7`.
    pub format: String,
    /// The `_SRGB` tag for formats that have one; null otherwise.
    pub srgb: Option<bool>,
    pub header: HeaderKind,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    pub cubemap: bool,
    /// Header findings of spec §5.8 (empty for a clean header).
    pub header_issues: Vec<String>,
    pub mips: Vec<DdsMipInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DdsUnreadable {
    pub kind: DdsInfoKind,
    pub schema: u32,
    pub path: String,
    pub error: String,
}

/// `sdc validate` on one file: the spec §5.8 verdict, or why the file cannot be read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DdsValidation {
    Validated(DdsVerdict),
    Unreadable(DdsValidationUnreadable),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DdsVerdict {
    pub kind: DdsValidationKind,
    pub schema: u32,
    pub path: String,
    pub readable: bool,
    /// The sub-type the file was judged as (given or inferred).
    pub subtype: Subtype,
    /// The alpha policy it was judged by; null when the format was not judged.
    pub alpha_policy: Option<AlphaPolicy>,
    /// False for a format sdc never writes: only its header was checked.
    pub judged: bool,
    pub note: Option<String>,
    pub passed: bool,
    pub summary: ValidationSummary,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DdsValidationUnreadable {
    pub kind: DdsValidationKind,
    pub schema: u32,
    pub path: String,
    pub readable: bool,
    pub passed: bool,
    pub error: String,
}
