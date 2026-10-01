//! runtime/mod.rs — runtime module organization
//! Groups the runtime value, scope, collection, and operator implementations used by evaluation.
//! Key components: collections, operations, scope, and value submodules.
pub(crate) mod arena;
pub(crate) mod collections;
pub(crate) mod files;
pub(crate) mod limits;
pub(crate) mod operations;
pub(crate) mod scope;
pub(crate) mod storage;
pub(crate) mod value;
