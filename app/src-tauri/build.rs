//! Records build provenance and validates distribution identity. The application reads its
//! version from Tauri's package info, the same source used by the bundle and updater.

mod build_identity;

use std::path::Path;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// "20 Oct 2026, 07:12 UTC" for seconds since the epoch.
fn built_at(secs: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since the epoch to a civil date (Howard Hinnant's days_from_civil, backwards).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{day} {} {year}, {:02}:{:02} UTC",
        MONTHS[(month - 1) as usize],
        rest / 3600,
        rest % 3600 / 60
    )
}

fn main() {
    // Again when the checked-out commit changes, not on every build.
    if let Some(dir) = git(&["rev-parse", "--git-dir"]) {
        let dir = Path::new(&dir);
        println!("cargo:rerun-if-changed={}", dir.join("HEAD").display());
        println!("cargo:rerun-if-changed={}", dir.join("packed-refs").display());
        if let Some(head) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={}", dir.join(head).display());
        }
    }
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    for key in [
        "OXBOW_BUILD_CHANNEL",
        "OXBOW_BUILD_VERSION",
        "OXBOW_BUILD_COMMIT",
        "TAURI_CONFIG",
        "OXBOW_UPDATER_TEST_ROOT",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    println!("cargo:rerun-if-changed=build_identity.rs");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string("tauri.conf.json").expect("read Tauri config"))
            .expect("parse Tauri config");
    let override_config: serde_json::Value = std::env::var("TAURI_CONFIG")
        .map(|text| serde_json::from_str(&text).expect("parse Tauri config override"))
        .unwrap_or_default();
    let version = override_config
        .get("version")
        .unwrap_or(&config["version"])
        .as_str()
        .expect("Tauri version is a string");
    if std::env::var_os("CARGO_FEATURE_UPDATER_TEST").is_some() {
        assert_eq!(
            override_config["identifier"], "app.oxbow.updater-test",
            "Updater tests require their own identifier"
        );
        assert!(
            matches!(version, "0.0.1" | "0.0.2"),
            "Updater test versions must stay outside distribution versions"
        );
        let key = override_config
            .pointer("/plugins/updater/pubkey")
            .and_then(serde_json::Value::as_str)
            .expect("Explicit updater test public key");
        assert!(
            !key.is_empty() && key != config["plugins"]["updater"]["pubkey"].as_str().unwrap(),
            "Updater tests must use a disposable signing key"
        );
        let root = std::env::var("OXBOW_UPDATER_TEST_ROOT").expect("Updater test fixture directory");
        println!("cargo:rustc-env=OXBOW_UPDATER_TEST_ROOT={root}");
        println!("cargo:rerun-if-changed={root}/certificate.pem");
    }
    let full_commit = git(&["rev-parse", "HEAD"]);
    let expected_version = std::env::var("OXBOW_BUILD_VERSION").ok();
    let expected_commit = std::env::var("OXBOW_BUILD_COMMIT").ok();
    let channel = std::env::var("OXBOW_BUILD_CHANNEL").ok();
    let identity = build_identity::BuildIdentity {
        version,
        commit: full_commit.as_deref(),
        expected_version: expected_version.as_deref(),
        expected_commit: expected_commit.as_deref(),
        channel: channel.as_deref(),
    };
    let channel = identity.validate().expect("distribution build identity");
    println!("cargo:rustc-env=OXBOW_BUILD_CHANNEL={channel}");
    let commit = match git(&["rev-parse", "--short=7", "HEAD"]) {
        Some(sha) if git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty()) => {
            format!("{sha}, modified")
        }
        Some(sha) => sha,
        None => "unknown commit".to_owned(),
    };
    let now = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64)
        });
    println!("cargo:rustc-env=OXBOW_COMMIT={commit}");
    println!("cargo:rustc-env=OXBOW_BUILT={}", built_at(now));

    tauri_build::build()
}
