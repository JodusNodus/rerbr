//! The physics step's per-step debug hook. Its tools are a keyboard camera, the orbit camera, a collision probe, wall
//! visitors and debug draws. They run only when one of the physics definition's debug flags is set. This port does not
//! run them; it covers the case where none is set, and `is_idle` says which case a call is.
