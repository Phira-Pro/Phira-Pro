prpr_l10n::tl_file!("multiplayer" mtl);

mod panel;
pub use panel::MPPanel;

#[cfg(feature = "local-mp")]
pub mod local;
