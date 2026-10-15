//! Why a parametric model could not be built, edited or read back.
//!
//! Every failure here names a feature and says what was wrong with it. A model that cannot be
//! rebuilt is a document a person has to fix, so "something went wrong" is not a message.

use cadcraft_solid::SolidError;

/// Everything that can go wrong in the parametric model.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ModelError {
    #[error("there is no feature called {0:?}")]
    NoSuchFeature(String),

    #[error("feature {0:?} already exists")]
    DuplicateFeature(String),

    #[error("a feature name may not be empty")]
    EmptyName,

    #[error("there is no parameter called {0:?}")]
    NoSuchParameter(String),

    #[error("parameter {0:?} is already defined")]
    DuplicateParameter(String),

    #[error("feature {feature:?} needs {count} inputs and has {given}")]
    WrongArity { feature: String, count: usize, given: usize },

    #[error("{name}: {source}")]
    Kernel { name: String, source: SolidError },

    #[error("{0}")]
    Expression(String),

    #[error("a feature may not depend on itself ({0:?})")]
    Cyclic(String),

    #[error("the model has no solid: nothing has been built yet")]
    NoSolid,

    #[error("this document was written by a newer version ({found}), and this build reads up to {supported}")]
    TooNew { found: u32, supported: u32 },

    #[error("this file is not a CADCraft 3D model")]
    NotAModel,

    #[error("the model's history is corrupt: {0}")]
    CorruptHistory(String),
}

impl ModelError {
    /// Name the feature a kernel failure belongs to, so the caller can point at it.
    pub fn in_feature(name: &str, source: SolidError) -> ModelError {
        ModelError::Kernel { name: name.to_string(), source }
    }
}
