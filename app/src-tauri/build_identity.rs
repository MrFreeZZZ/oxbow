//! Distribution builds must describe the same version and commit as the Tauri bundle.

pub struct BuildIdentity<'a> {
    pub version: &'a str,
    pub commit: Option<&'a str>,
    pub expected_version: Option<&'a str>,
    pub expected_commit: Option<&'a str>,
    pub channel: Option<&'a str>,
}

impl BuildIdentity<'_> {
    pub fn validate(&self) -> Result<&str, String> {
        let channel = self.channel.unwrap_or("preview");
        if !matches!(channel, "release" | "preview") {
            return Err("OXBOW_BUILD_CHANNEL must be release or preview".into());
        }
        if let Some(version) = self.expected_version {
            let (core, preview) = version
                .split_once("-preview.")
                .map_or((version, None), |(v, n)| (v, Some(n)));
            let numeric = |part: &str| {
                !part.is_empty()
                    && part.bytes().all(|c| c.is_ascii_digit())
                    && part.parse::<u64>().is_ok()
                    && (part.len() == 1 || !part.starts_with('0'))
            };
            if core.split('.').count() != 3
                || !core.split('.').all(numeric)
                || preview.is_some_and(|n| channel != "preview" || !numeric(n))
            {
                return Err("OXBOW_BUILD_VERSION must be a stable or Preview SemVer version".into());
            }
            if self.version != version {
                return Err(format!(
                    "Tauri bundle version {} differs from distribution version {version}",
                    self.version
                ));
            }
            if self.channel.is_none() || self.expected_commit.is_none() {
                return Err("Distribution builds require an explicit channel and commit".into());
            }
        }
        if let Some(commit) = self.expected_commit
            && (commit.len() != 40
                || !commit.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                || self.commit != Some(commit))
        {
            return Err("The checked-out commit differs from OXBOW_BUILD_COMMIT".into());
        }
        Ok(channel)
    }
}

pub fn validate_release_context(base: &str, distance: &str, required: bool) -> Result<(), String> {
    if !required && base.is_empty() && distance.is_empty() {
        return Ok(());
    }
    let numeric = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|c| c.is_ascii_digit())
            && part.parse::<u64>().is_ok()
            && (part.len() == 1 || !part.starts_with('0'))
    };
    if base.split('.').count() != 3 || !base.split('.').all(numeric) || !numeric(distance) {
        return Err("Builds require a resolved numeric base Release and commit distance".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn distribution() -> BuildIdentity<'static> {
        BuildIdentity {
            version: "0.1.550",
            commit: Some(SHA),
            expected_version: Some("0.1.550"),
            expected_commit: Some(SHA),
            channel: Some("release"),
        }
    }

    #[test]
    fn distribution_version_must_match_the_bundle() {
        let mut build = distribution();
        assert_eq!(build.validate().unwrap(), "release");
        build.version = "0.1.0";
        assert!(build.validate().unwrap_err().contains("bundle version"));
    }

    #[test]
    fn distribution_needs_an_explicit_channel_and_the_full_matching_commit() {
        let mut build = distribution();
        build.channel = None;
        assert!(build.validate().is_err());
        build.channel = Some("preview");
        build.expected_commit = None;
        assert!(build.validate().is_err());
        build.expected_commit = Some(SHA);
        build.commit = Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert!(build.validate().is_err());
        build.commit = Some(SHA);
        build.expected_commit = Some("aaaaaaa");
        assert!(build.validate().is_err());
    }

    #[test]
    fn versions_and_channels_are_strictly_validated() {
        let mut build = distribution();
        for version in [
            "0.1.550-preview",
            "2026.9.0-preview.01",
            "2026.9.0-beta.1",
            "0.1.0550",
            "0.1",
            "0.1.+1",
            "0.1.-1",
        ] {
            build.expected_version = Some(version);
            build.version = version;
            assert!(build.validate().is_err(), "{version}");
        }
        let mut build = distribution();
        build.channel = Some("nightly");
        assert!(build.validate().is_err());
    }

    #[test]
    fn preview_preserves_the_full_semver() {
        let mut build = distribution();
        build.version = "2026.9.0-preview.748";
        build.expected_version = Some(build.version);
        build.channel = Some("preview");
        assert_eq!(build.validate().unwrap(), "preview");
        build.channel = Some("release");
        assert!(build.validate().is_err());
    }

    #[test]
    fn about_metadata_requires_both_resolved_values() {
        assert!(validate_release_context("", "", false).is_ok());
        assert!(validate_release_context("2026.8.0", "52", true).is_ok());
        for (base, distance) in [
            ("", ""),
            ("2026.8.0", ""),
            ("", "52"),
            ("2026.8.0-preview.1", "52"),
            ("2026.08.0", "52"),
            ("2026.8.0", "+1"),
        ] {
            assert!(validate_release_context(base, distance, true).is_err());
        }
    }

    #[test]
    fn development_uses_preview_without_claiming_a_distribution_version() {
        let build = BuildIdentity {
            version: "0.1.0",
            commit: None,
            expected_version: None,
            expected_commit: None,
            channel: None,
        };
        assert_eq!(build.validate().unwrap(), "preview");
    }
}
