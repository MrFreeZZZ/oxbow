use super::{ConfigLine, FileCheck, Tree, config_lines};

pub(super) fn check_gitmodules(text: &str, tree: Option<&Tree>, check: &mut FileCheck) {
    let lines = match config_lines(text) {
        Ok(lines) => lines,
        Err(no) => {
            let line = text.split('\n').nth(no.saturating_sub(1)).unwrap_or("");
            check.error(
                no,
                line,
                (0, line.trim_end().len()),
                "Git can’t read this line, so the submodule commands stop.",
                None,
            );
            return;
        }
    };
    // section → (header line, path, url)
    let mut modules: Vec<(String, usize, &str, Option<String>, bool)> = Vec::new();
    for (no, line, section, parsed) in &lines {
        match parsed {
            ConfigLine::Section(name) if name.starts_with("submodule.") => {
                modules.push((name.clone(), *no, line, None, false))
            }
            ConfigLine::Key { name, value, .. } => {
                if let Some(module) = modules.iter_mut().rev().find(|m| m.0 == *section) {
                    match name.as_str() {
                        "path" => module.3 = value.map(str::to_owned),
                        "url" => module.4 = value.is_some_and(|v| !v.is_empty()),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    for (name, no, line, path, url) in modules {
        let span = (0, line.trim_end().len());
        let name = name.trim_start_matches("submodule.");
        match path {
            None => check.warn(no, line, span, format!("Submodule `{name}` has no `path`."), None),
            Some(path) => {
                if let Some(tree) = tree
                    && !tree.gitlinks.contains(path.trim_end_matches('/'))
                {
                    check.warn(
                        no,
                        line,
                        span,
                        format!("No submodule at `{path}` in the repository."),
                        None,
                    );
                }
            }
        }
        if !url {
            check.warn(
                no,
                line,
                span,
                format!("Submodule `{name}` has no `url`, so it can’t be cloned."),
                None,
            );
        }
    }
}
