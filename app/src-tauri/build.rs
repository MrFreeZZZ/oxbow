//! Works out what the About window shows: the version, with the number of commits as its last
//! part so it grows with every merge; the commit the build is of; and when it was built.

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

    let base = env!("CARGO_PKG_VERSION")
        .splitn(3, '.')
        .take(2)
        .collect::<Vec<_>>()
        .join(".");
    let version = match git(&["rev-list", "--count", "HEAD"]) {
        Some(count) => format!("{base}.{count}"),
        None => env!("CARGO_PKG_VERSION").to_owned(),
    };
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
    println!("cargo:rustc-env=OXBOW_VERSION={version}");
    println!("cargo:rustc-env=OXBOW_COMMIT={commit}");
    println!("cargo:rustc-env=OXBOW_BUILT={}", built_at(now));

    tauri_build::build()
}
