//! Publish a validated chart only after extraction has succeeded.
use anyhow::{Context, Result};
use std::path::Path;

pub fn publish(staged: &Path, destination: &Path) -> Result<()> {
    let previous = destination.with_file_name(format!(".previous-{}-{}", destination.file_name().unwrap().to_string_lossy(), uuid::Uuid::new_v4()));
    let had_previous = destination.exists();
    if had_previous {
        std::fs::rename(destination, &previous).context("failed to preserve the previous chart")?;
    }
    if let Err(error) = std::fs::rename(staged, destination) {
        if had_previous {
            std::fs::rename(&previous, destination).context("failed to restore the previous chart")?;
        }
        return Err(error).context("failed to install the validated chart");
    }
    // Preserve old/partial contents rather than discarding a user's files.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_publish_restores_the_existing_chart() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("12");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("old"), b"kept").unwrap();
        assert!(publish(&root.path().join("missing"), &target).is_err());
        assert_eq!(std::fs::read(target.join("old")).unwrap(), b"kept");
    }

    #[test]
    fn a_successful_publish_preserves_the_previous_contents() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("12");
        let stage = root.path().join("staged");
        std::fs::create_dir(&target).unwrap();
        std::fs::create_dir(&stage).unwrap();
        std::fs::write(target.join("old"), b"kept").unwrap();
        std::fs::write(stage.join("info.yml"), b"new").unwrap();
        publish(&stage, &target).unwrap();
        assert_eq!(std::fs::read(target.join("info.yml")).unwrap(), b"new");
        let backup = std::fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .find(|it| it.file_name().to_string_lossy().starts_with(".previous-"))
            .unwrap();
        assert_eq!(std::fs::read(backup.path().join("old")).unwrap(), b"kept");
    }
}
