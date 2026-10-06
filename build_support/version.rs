//! Shared by the library and desktop/iOS build scripts; independent of host OS.
#![allow(dead_code)] // Each build entry uses only the relevant parts of this module.
use std::{env, fs, path::Path};

pub struct Version {
    pub base: String,
    pub pro: String,
    pub ios: String,
    pub flash: String,
    pub build: u32,
}

impl Version {
    pub fn read(root: &Path) -> Self {
        let path = root.join("version.json");
        println!("cargo:rerun-if-changed={}", path.display());
        println!("cargo:rerun-if-changed={}", root.join("build_support/version.rs").display());
        let values: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("cannot read version.json")).expect("invalid version.json");
        let object = values.as_object().expect("version.json must be an object");
        assert!(
            object.len() == 4
                && ["base_version", "pro_revision", "flash_revision", "build_number"]
                    .iter()
                    .all(|key| object.contains_key(*key)),
            "unexpected version.json fields"
        );
        let base = values["base_version"].as_str().expect("base_version must be a string");
        let parts: Vec<_> = base.split('.').collect();
        assert!(
            parts.len() == 3
                && parts
                    .iter()
                    .all(|part| { !part.is_empty() && part.bytes().all(|c| c.is_ascii_digit()) && (part.len() == 1 || !part.starts_with('0')) }),
            "base_version must be a numeric major.minor.patch version"
        );
        let number = |key: &str| {
            let value = values[key]
                .as_u64()
                .filter(|n| (1..=2_100_000_000).contains(n))
                .unwrap_or_else(|| panic!("invalid {key}"));
            value as u32
        };
        assert_eq!(base, env::var("CARGO_PKG_VERSION").unwrap(), "Cargo version is stale; run python scripts/version.py sync");
        Self {
            base: base.into(),
            pro: format!("{base}-pro.{}", number("pro_revision")),
            ios: format!("{}.{}.{}", parts[0], parts[1], number("pro_revision")),
            flash: format!("flash.{}", number("flash_revision")),
            build: number("build_number"),
        }
    }

    pub fn emit(&self) {
        for (key, value) in [
            ("PHIRA_PRO_VERSION", self.pro.clone()),
            ("PHIRA_PRO_VERSION_TAG", format!("v{}", self.pro)),
            ("PHIRA_FLASH_VERSION", self.flash.clone()),
            ("PHIRA_FLASH_VERSION_TAG", format!("v{}", self.flash)),
            ("PHIRA_BUILD_NUMBER", self.build.to_string()),
        ] {
            println!("cargo:rustc-env={key}={value}");
        }
    }

    pub fn check_xcode(&self, root: &Path) {
        let path = root.join("xcode/Version.xcconfig");
        println!("cargo:rerun-if-changed={}", path.display());
        let expected = format!(
            "// Generated from version.json by scripts/version.py sync. Do not edit.\nMARKETING_VERSION = {}\nCURRENT_PROJECT_VERSION = {}\nPHIRA_PRO_VERSION = {}\n",
            self.ios, self.build, self.pro
        );
        let actual = fs::read_to_string(path).expect("missing xcode/Version.xcconfig; run python scripts/version.py sync");
        assert_eq!(actual.replace("\r\n", "\n"), expected, "Xcode version is stale; run python scripts/version.py sync");
    }
}
