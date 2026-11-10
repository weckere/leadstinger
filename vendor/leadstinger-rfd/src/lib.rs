//! LeadStinger's file dialog: `rfd::FileDialog` itself on desktop, and on iPadOS (where
//! rfd has no backend) a stand-in with the same calls (docs/IPAD-PLAN.md).
#[cfg(not(target_os = "ios"))]
pub use rfd_desktop::*;

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub use ios::*;
