//! The committed operator reference has to match the operators.
//!
//! Generated docs solve drift only if the generated file is actually
//! regenerated. This makes a stale `docs/OPERATORS.md` a failing build rather
//! than something noticed months later by somebody reading a page that lies.

use std::path::PathBuf;

fn committed() -> PathBuf {
    // The workspace root, from this crate's directory.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/OPERATORS.md")
}

#[test]
fn the_committed_reference_is_up_to_date() {
    let path = committed();
    let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}\nRun `otd docs --out docs/OPERATORS.md`",
            path.display()
        )
    });
    let generated = otd_engine::docs::reference(&otd_engine::registry());

    if on_disk != generated {
        // Say what changed rather than dumping a thousand lines.
        let changed = on_disk
            .lines()
            .zip(generated.lines())
            .filter(|(a, b)| a != b)
            .count();
        panic!(
            "docs/OPERATORS.md is out of date ({changed} differing lines, \
             {} vs {} lines). Run:\n\n    cargo run -p otd-cli -- docs --out docs/OPERATORS.md\n",
            on_disk.lines().count(),
            generated.lines().count()
        );
    }
}
