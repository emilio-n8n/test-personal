//! Saving and loading a model document.
//!
//! A `.ccm` file holds the *history*, not the geometry: parameters, features and their sketches.
//! Opening one rebuilds it, exactly as if it had just been typed. That is the whole point of a
//! parametric document — and it has two consequences worth stating plainly:
//!
//! - The geometry in a file can never disagree with the features in it, because it is not stored.
//!   An old file opens with whatever the current kernel makes of it, and a new file opens on an
//!   old build with a version error rather than with geometry it cannot check.
//! - A file whose features no longer build still *loads*. The tree, the parameters and the last
//!   error come back, and the person who owns the document can fix it. Refusing to open a file
//!   because one dimension is wrong would be the worst possible answer.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    error::ModelError,
    feature::{Feature, Model, Param},
    rebuild::{self, Rebuild},
};

/// The `.ccm` magic, so a wrong file is a clear error rather than a parse failure.
const MAGIC: &str = "CADCraft model";

/// The newest format this build reads.
pub const FORMAT_VERSION: u32 = 1;

/// The on-disk form. Everything here is stable, public and versioned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub magic: String,
    pub version: u32,
    #[serde(default)]
    pub parameters: BTreeMap<String, Param>,
    #[serde(default)]
    pub features: Vec<Feature>,
}

impl Document {
    pub fn new(parameters: BTreeMap<String, Param>, features: Vec<Feature>) -> Document {
        Document { magic: MAGIC.to_string(), version: FORMAT_VERSION, parameters, features }
    }

    /// Parse a document and rebuild it.
    ///
    /// A model whose features do not build still comes back: `rebuild.problems` says which, and
    /// `model.solid` is whatever last worked.
    pub fn into_model(self) -> Result<(Model, Rebuild), ModelError> {
        if self.magic != MAGIC {
            return Err(ModelError::NotAModel);
        }
        if self.version > FORMAT_VERSION {
            return Err(ModelError::TooNew { found: self.version, supported: FORMAT_VERSION });
        }
        let mut model = Model { parameters: self.parameters, features: self.features, records: Vec::new() };
        // Two features with the same name would make a reference ambiguous, so a file that has one
        // is refused rather than quietly renamed.
        for (i, f) in model.features.iter().enumerate() {
            if model.features[..i].iter().any(|e| e.name == f.name) {
                return Err(ModelError::DuplicateFeature(f.name.clone()));
            }
        }
        let result = rebuild::rebuild(&mut model, None);
        Ok((model, result))
    }
}

impl Model {
    /// The document this model is. Round-trips exactly.
    pub fn to_document(&self) -> Document {
        Document::new(self.parameters.clone(), self.features.clone())
    }

    pub fn save_json(&self) -> String {
        serde_json::to_string_pretty(&self.to_document())
            .unwrap_or_else(|e| format!(r#"{{"magic":"{MAGIC}","version":{FORMAT_VERSION},"error":{e:?}}}"#))
    }

    /// Load a document and rebuild it.
    pub fn load_json(text: &str) -> Result<(Model, Rebuild), ModelError> {
        let doc: Document = serde_json::from_str(text).map_err(|e| ModelError::CorruptHistory(e.to_string()))?;
        doc.into_model()
    }
}
