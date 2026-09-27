//! The agent skills in `.claude/skills` teach only operators that exist.
//!
//! A skill is prose an agent follows literally. One that names a renamed or
//! removed operator produces a patch that fails validation — or worse, a
//! confident explanation of a feature that is not there. The operator names
//! all share a family suffix, so every one can be checked.

use std::path::PathBuf;

const FAMILIES: &[&str] = &["TOP", "CHOP", "SOP", "DAT", "MAT", "COMP"];

fn skills() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.claude/skills");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| {
            let path = e.ok()?.path().join("SKILL.md");
            let dir = path.parent()?.file_name()?.to_str()?.to_owned();
            Some((dir, std::fs::read_to_string(path).ok()?))
        })
        .collect();
    out.sort();
    out
}

/// Words like `lagCHOP`: a lowercase start and a family suffix.
fn operator_names(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.starts_with(|c: char| c.is_ascii_lowercase()))
        .filter(|w| FAMILIES.iter().any(|f| w.len() > f.len() && w.ends_with(f)))
        .collect()
}

#[test]
fn every_operator_a_skill_names_is_registered() {
    let reg = otd_engine::registry();
    let skills = skills();
    assert!(skills.len() >= 9, "found only {} skills", skills.len());
    let mut missing = Vec::new();
    for (name, text) in &skills {
        for op in operator_names(text) {
            if reg.get(op).is_none() {
                missing.push(format!("{name}: {op}"));
            }
        }
    }
    assert!(missing.is_empty(), "skills name operators that do not exist:\n{}", missing.join("\n"));
}

#[test]
fn every_skill_has_frontmatter_matching_its_folder() {
    for (dir, text) in skills() {
        let front = text
            .strip_prefix("---\n")
            .and_then(|t| t.split_once("\n---\n"))
            .map(|(f, _)| f)
            .unwrap_or_else(|| panic!("{dir}: no frontmatter"));
        assert!(front.lines().any(|l| l == format!("name: {dir}")), "{dir}: name must match folder");
        let desc = front.lines().find_map(|l| l.strip_prefix("description: "));
        assert!(desc.is_some_and(|d| d.len() > 40 && d.contains("Use ")), "{dir}: description must say when to use it");
    }
}

#[test]
fn the_scanner_finds_operators_and_ignores_prose() {
    assert_eq!(operator_names("wire `lagCHOP` into a TOP, then noiseSOP."), ["lagCHOP", "noiseSOP"]);
    assert!(operator_names("TOP CHOP sTD2DInputs").is_empty());
}
