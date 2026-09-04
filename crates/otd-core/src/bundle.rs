//! Exporting a project as a self-contained folder.
//!
//! A project is not one file. It references `.otdc` component files, and those
//! live wherever the author happened to keep them — a scratch folder, another
//! project's directory, a Dropbox path with their name in it. That is right
//! for authoring, where the whole point of a shared component is that two
//! projects use one copy of it, and wrong for a show machine, where anything
//! not in the folder you copied across is a file that will be missing at 8pm.
//!
//! An export copies every referenced component in beside the project and
//! rewrites the references to be relative to it. The result opens the same way
//! from any directory on any machine — which is what [`Project::open`] and the
//! graph's base directory exist to make true.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::graph::{Graph, OpRegistry};
use crate::project::{Project, ProjectError};

/// Where components go inside a bundle.
pub const COMPONENT_DIR: &str = "components";

/// Where movies, images and audio go inside a bundle.
pub const MEDIA_DIR: &str = "media";

/// What an export produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub project: PathBuf,
    /// The component files copied in, in the order they were found.
    pub components: Vec<PathBuf>,
    /// The movies, images and audio copied in.
    pub media: Vec<PathBuf>,
    /// References that could not be read, as `(node path, file, reason)`.
    ///
    /// A missing component is reported rather than thrown: an artist who is
    /// mid-reorganisation should get a list of what to fix, not one error and
    /// no bundle.
    pub missing: Vec<(String, String, String)>,
}

/// Write `graph` and everything it references into `dir`.
///
/// The project lands at `dir/<name>.otd` and components at
/// `dir/components/<file>.otdc`. Two components with the same file name from
/// different folders are kept apart by numbering, because collapsing them
/// would silently point one instance at the other's network.
pub fn export(
    graph: &Graph,
    registry: &OpRegistry,
    fps: f64,
    dir: impl AsRef<Path>,
    name: &str,
) -> Result<Bundle, ProjectError> {
    let dir = dir.as_ref();
    std::fs::create_dir_all(dir)?;

    let mut copied: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut used_names: Vec<String> = Vec::new();
    let mut components = Vec::new();
    let mut missing = Vec::new();

    // The rewritten references are collected by node path and applied to the
    // *file* rather than to the graph. Exporting must not disturb the project
    // the artist is still working in, where those references are deliberately
    // pointing at shared files outside the folder.
    let mut rewritten: BTreeMap<String, String> = BTreeMap::new();

    for id in graph.walk() {
        let Some(file) = graph.node(id).external.clone() else {
            continue;
        };
        let source = graph.resolve_external(&file);

        // The same component used twice is one file in the bundle, and both
        // instances point at it.
        if let Some(existing) = copied.get(&source) {
            rewritten.insert(graph.path(id), existing.clone());
            continue;
        }

        let stem = Path::new(&file)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "component".into());
        let mut unique = stem.clone();
        let mut n = 1;
        while used_names.contains(&unique) {
            unique = format!("{stem}{n}");
            n += 1;
        }
        used_names.push(unique.clone());

        let relative = format!("{COMPONENT_DIR}/{unique}.otdc");
        let target = dir.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::copy(&source, &target) {
            Ok(_) => {
                copied.insert(source, relative.clone());
                components.push(target);
                rewritten.insert(graph.path(id), relative);
            }
            Err(e) => missing.push((graph.path(id), file, e.to_string())),
        }
    }

    // ---- Media. A project that references a movie is as dependent on it as
    // one that references a component, and by exactly the same argument: the
    // authoring path is somebody's scratch folder, and the show machine has
    // no such folder.
    let mut media = Vec::new();
    let mut media_names: Vec<String> = Vec::new();
    let mut media_copied: BTreeMap<PathBuf, String> = BTreeMap::new();
    // `(node path, param key) -> relative path`, applied to the file rather
    // than to the graph, for the same reason component references are.
    let mut media_rewritten: BTreeMap<(String, String), String> = BTreeMap::new();

    for id in graph.walk() {
        let node = graph.node(id);
        for (key, param) in &node.params {
            if !param.is_file_ref() {
                continue;
            }
            let value = param.value.as_str();
            if value.trim().is_empty() {
                continue;
            }
            let source = graph.resolve_external(value.trim());
            if let Some(existing) = media_copied.get(&source) {
                media_rewritten.insert((graph.path(id), key.clone()), existing.clone());
                continue;
            }

            let name = Path::new(value.trim())
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "media".into());
            let (stem, extension) = match name.rsplit_once('.') {
                Some((s, e)) => (s.to_string(), format!(".{e}")),
                None => (name.clone(), String::new()),
            };
            let mut unique = format!("{stem}{extension}");
            let mut n = 1;
            while media_names.contains(&unique) {
                unique = format!("{stem}{n}{extension}");
                n += 1;
            }
            media_names.push(unique.clone());

            let relative = format!("{MEDIA_DIR}/{unique}");
            let target = dir.join(&relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            match std::fs::copy(&source, &target) {
                Ok(_) => {
                    media_copied.insert(source, relative.clone());
                    media.push(target);
                    media_rewritten.insert((graph.path(id), key.clone()), relative);
                }
                Err(e) => missing.push((graph.path(id), value, e.to_string())),
            }
        }
    }

    let mut out = Project::from_graph(graph, registry, fps);
    for entry in &mut out.nodes {
        if let Some(relative) = rewritten.get(&entry.path) {
            entry.external = Some(relative.clone());
        }
        for (key, param) in entry.params.iter_mut() {
            if let Some(relative) = media_rewritten.get(&(entry.path.clone(), key.clone())) {
                param.value = crate::value::Value::Str(relative.clone());
            }
        }
    }
    let project = dir.join(format!("{name}.otd"));
    out.save(&project)?;

    Ok(Bundle {
        project,
        components,
        media,
        missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::Component;
    use crate::graph::{Connector, Family, OpDef};
    use crate::param::Param;

    pub(super) fn registry() -> OpRegistry {
        fn none() -> indexmap::IndexMap<String, Param> {
            indexmap::IndexMap::new()
        }
        let mut r = OpRegistry::new();
        r.register(OpDef {
            type_name: "container",
            label: "Container",
            family: Family::Comp,
            inputs: &[],
            summary: "",
            time_dependent: false,
            params: none,
            connector: Connector::None,
        });
        fn with_file() -> indexmap::IndexMap<String, Param> {
            let mut m = indexmap::IndexMap::new();
            m.insert("file".into(), Param::str("").as_file_ref());
            m
        }
        r.register(OpDef {
            type_name: "player",
            label: "Player",
            family: Family::Top,
            inputs: &[],
            summary: "",
            time_dependent: true,
            params: with_file,
            connector: Connector::None,
        });
        r.register(OpDef {
            type_name: "pass",
            label: "Pass",
            family: Family::Top,
            inputs: &["in"],
            summary: "",
            time_dependent: false,
            params: none,
            connector: Connector::None,
        });
        r
    }

    pub(super) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("otd-bundle-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A `.otdc` holding one operator, saved somewhere the bundle is not.
    fn write_component(dir: &Path, name: &str) -> PathBuf {
        let reg = registry();
        let mut g = Graph::new();
        let comp = g
            .create(g.root(), reg.get("container").unwrap(), None)
            .unwrap();
        g.create(comp, reg.get("pass").unwrap(), None).unwrap();
        let path = dir.join(format!("{name}.otdc"));
        Component::from_graph(&g, comp, &reg)
            .unwrap()
            .save(&path)
            .unwrap();
        path
    }

    #[test]
    fn an_exported_bundle_opens_from_anywhere() {
        let reg = registry();
        let elsewhere = scratch("elsewhere");
        let source = write_component(&elsewhere, "meter");

        let mut g = Graph::new();
        let comp = g
            .create(g.root(), reg.get("container").unwrap(), None)
            .unwrap();
        g.attach_external(comp, &source.to_string_lossy(), &reg)
            .unwrap();

        let out = scratch("out");
        let bundle = export(&g, &reg, 60.0, &out, "show").unwrap();
        assert!(bundle.missing.is_empty(), "{:?}", bundle.missing);
        assert_eq!(bundle.components.len(), 1);
        assert!(out.join("components/meter.otdc").exists());

        // The point of the exercise: the component the bundle now depends on
        // is gone from where it was authored, and the bundle still opens.
        std::fs::remove_dir_all(&elsewhere).unwrap();
        let loaded = Project::open(&bundle.project, &reg).unwrap();
        let id = loaded.find("/container1").unwrap();
        assert_eq!(
            loaded.node(id).external.as_deref(),
            Some("components/meter.otdc"),
            "the reference is relative, so the folder can move"
        );
        assert_eq!(
            loaded.node(id).children.len(),
            1,
            "the component's network came back with it"
        );
    }

    #[test]
    fn exporting_does_not_disturb_the_project_being_worked_in() {
        let reg = registry();
        let elsewhere = scratch("keep-elsewhere");
        let source = write_component(&elsewhere, "meter");

        let mut g = Graph::new();
        let comp = g
            .create(g.root(), reg.get("container").unwrap(), None)
            .unwrap();
        g.attach_external(comp, &source.to_string_lossy(), &reg)
            .unwrap();

        export(&g, &reg, 60.0, scratch("keep-out"), "show").unwrap();
        assert_eq!(
            g.node(comp).external.as_deref(),
            Some(source.to_string_lossy().as_ref()),
            "the open project still points at the shared file it is editing"
        );
    }

    #[test]
    fn one_component_used_twice_is_copied_once() {
        let reg = registry();
        let elsewhere = scratch("twice-src");
        let source = write_component(&elsewhere, "meter");

        let mut g = Graph::new();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let c = g
                .create(g.root(), reg.get("container").unwrap(), None)
                .unwrap();
            g.attach_external(c, &source.to_string_lossy(), &reg)
                .unwrap();
            ids.push(c);
        }

        let out = scratch("twice-out");
        let bundle = export(&g, &reg, 60.0, &out, "show").unwrap();
        assert_eq!(bundle.components.len(), 1, "one file, two instances");

        let loaded = Project::open(&bundle.project, &reg).unwrap();
        for path in ["/container1", "/container2"] {
            let id = loaded.find(path).unwrap();
            assert_eq!(
                loaded.node(id).external.as_deref(),
                Some("components/meter.otdc"),
                "{path} shares the one copy"
            );
        }
    }

    #[test]
    fn two_different_components_with_the_same_name_do_not_collide() {
        let reg = registry();
        let a_dir = scratch("collide-a");
        let b_dir = scratch("collide-b");
        let a = write_component(&a_dir, "meter");
        let b = write_component(&b_dir, "meter");

        let mut g = Graph::new();
        for source in [&a, &b] {
            let c = g
                .create(g.root(), reg.get("container").unwrap(), None)
                .unwrap();
            g.attach_external(c, &source.to_string_lossy(), &reg)
                .unwrap();
        }

        let out = scratch("collide-out");
        let bundle = export(&g, &reg, 60.0, &out, "show").unwrap();
        // Two distinct networks that happen to share a file name must stay
        // two files — overwriting one with the other would point an instance
        // at somebody else's component and look like it worked.
        assert_eq!(bundle.components.len(), 2, "{:?}", bundle.components);
        assert!(out.join("components/meter.otdc").exists());
        assert!(out.join("components/meter1.otdc").exists());
    }

    #[test]
    fn a_missing_component_is_listed_rather_than_aborting_the_export() {
        let reg = registry();
        let mut g = Graph::new();
        let comp = g
            .create(g.root(), reg.get("container").unwrap(), None)
            .unwrap();
        // Reference a file that is not there, the way a half-reorganised
        // project does.
        g.node_mut_quiet(comp).external = Some("/nowhere/gone.otdc".into());

        let out = scratch("missing-out");
        let bundle = export(&g, &reg, 60.0, &out, "show").unwrap();
        assert_eq!(bundle.missing.len(), 1);
        assert_eq!(bundle.missing[0].1, "/nowhere/gone.otdc");
        assert!(
            bundle.project.exists(),
            "the rest of the bundle is still written, so the list is actionable"
        );
    }
}

#[cfg(test)]
mod media_tests {
    use super::tests::*;
    use super::*;
    use crate::value::Value;

    /// A movie is as much a dependency as a component, and by the same
    /// argument: the authoring path is somebody's scratch folder.
    #[test]
    fn a_bundle_brings_the_media_with_it_and_rewrites_the_paths() {
        let reg = registry();
        let elsewhere = scratch("media-source");
        let clip = elsewhere.join("plate.mov");
        std::fs::write(&clip, b"not really a movie, but a real file").unwrap();

        let mut g = Graph::new();
        let a = g
            .create(g.root(), reg.get("player").unwrap(), Some("a"))
            .unwrap();
        let b = g
            .create(g.root(), reg.get("player").unwrap(), Some("b"))
            .unwrap();
        g.set_param(a, "file", Value::Str(clip.to_string_lossy().into()))
            .unwrap();
        // The same file on two nodes is one copy, and both point at it.
        g.set_param(b, "file", Value::Str(clip.to_string_lossy().into()))
            .unwrap();

        let out = scratch("media-out");
        let bundle = export(&g, &reg, 60.0, &out, "show").unwrap();
        assert!(bundle.missing.is_empty(), "{:?}", bundle.missing);
        assert_eq!(bundle.media.len(), 1, "one file, copied once");
        assert!(out.join("media/plate.mov").exists());

        // The source folder is gone; the bundle still knows where its movie is.
        std::fs::remove_dir_all(&elsewhere).unwrap();
        let loaded = Project::open(&bundle.project, &reg).unwrap();
        for name in ["/a", "/b"] {
            let id = loaded.find(name).unwrap();
            assert_eq!(
                loaded.node(id).param("file").unwrap().value.as_str(),
                "media/plate.mov",
                "{name} should point inside the bundle"
            );
        }
        assert_eq!(
            loaded.resolve_external("media/plate.mov"),
            out.join("media/plate.mov"),
            "and that relative path resolves against the bundle"
        );
    }

    #[test]
    fn exporting_does_not_rewrite_the_project_being_worked_in() {
        let reg = registry();
        let elsewhere = scratch("media-keep");
        let clip = elsewhere.join("plate.mov");
        std::fs::write(&clip, b"x").unwrap();

        let mut g = Graph::new();
        let a = g
            .create(g.root(), reg.get("player").unwrap(), Some("a"))
            .unwrap();
        g.set_param(a, "file", Value::Str(clip.to_string_lossy().into()))
            .unwrap();

        export(&g, &reg, 60.0, scratch("media-keep-out"), "show").unwrap();
        assert_eq!(
            g.node(a).param("file").unwrap().value.as_str(),
            clip.to_string_lossy(),
            "the open project still points at the file the artist is using"
        );
    }

    #[test]
    fn a_missing_movie_is_listed_rather_than_thrown() {
        let reg = registry();
        let mut g = Graph::new();
        let a = g
            .create(g.root(), reg.get("player").unwrap(), Some("a"))
            .unwrap();
        g.set_param(a, "file", Value::Str("/no/such/clip.mov".into()))
            .unwrap();

        // An artist mid-reorganisation should get a list of what to fix, not
        // one error and no bundle.
        let bundle = export(&g, &reg, 60.0, scratch("media-missing"), "show").unwrap();
        assert!(bundle.project.exists());
        assert_eq!(bundle.missing.len(), 1);
        assert!(bundle.missing[0].1.contains("clip.mov"));
    }

    #[test]
    fn an_empty_file_parameter_is_not_a_dependency() {
        let reg = registry();
        let mut g = Graph::new();
        g.create(g.root(), reg.get("player").unwrap(), Some("a"))
            .unwrap();
        let bundle = export(&g, &reg, 60.0, scratch("media-empty"), "show").unwrap();
        assert!(bundle.media.is_empty());
        assert!(bundle.missing.is_empty());
    }
}
