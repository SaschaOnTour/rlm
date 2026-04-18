// Slice 3.8 moved the contents of this module to
// `crate::application::index::*`. The glob re-export includes the
// `staleness` submodule so `crate::indexer::staleness::...` paths
// keep resolving.
pub use crate::application::index::*;
