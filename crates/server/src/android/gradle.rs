//! What the app module's Gradle build file says: the build variants the app can be built as,
//! and the id each one installs under.
//!
//! The file is read as text, never run, so only what is written in it is found: a flavor made in
//! a loop, or an id computed from a property, is not. A project then builds as `debug` and
//! `release`, which every Android project has.

// Rust guideline compliant 2026-10-09

use std::path::PathBuf;

use super::types::{Framework, MobileProject, Variant};
use crate::{Error, Server};

/// Where the variant last picked for a project is kept in the app's state, by project id.
const VARIANT_KEY: &str = "android-variant:";

/// The most variants offered, so a project with many flavor dimensions keeps a menu that fits.
const VARIANT_LIMIT: usize = 48;

/// Names inside `buildTypes` and `productFlavors` that configure every entry and name none.
const NOT_ENTRIES: [&str; 4] = ["all", "configureEach", "each", "forEach"];

/// The folder of the module that is the app.
fn app_folder(project: &MobileProject) -> PathBuf {
    match project.framework {
        Framework::Flutter | Framework::ReactNative | Framework::Expo => {
            project.root.join("android").join("app")
        }
        Framework::KotlinMultiplatform | Framework::NativeAndroid => {
            let mut folder = project.root.clone();
            for part in project.module.iter().flat_map(|module| module.split(':')) {
                folder.push(part);
            }
            folder
        }
    }
}

/// The text of the app module's build file.
fn build_file(project: &MobileProject) -> Option<String> {
    let folder = app_folder(project);
    std::fs::read_to_string(folder.join("build.gradle.kts"))
        .or_else(|_| std::fs::read_to_string(folder.join("build.gradle")))
        .ok()
}

/// The quoted value on the first line of `text` that starts with `key`, as in
/// `applicationId = "com.example.shop"`. None when the value is not a literal.
fn quoted_after(text: &str, key: &str) -> Option<String> {
    text.lines().map(str::trim).find_map(|line| {
        let rest = match line.strip_prefix(key) {
            Some(rest) => rest,
            // A manifest's attribute may share its line with others.
            None if key.ends_with('=') => &line[line.find(key)? + key.len()..],
            None => return None,
        };
        // `applicationIdSuffix` starts with `applicationId` and is another setting.
        if rest.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return None;
        }
        let value = rest.split(['"', '\'']).nth(1)?;
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// What is between the braces of the first block named `name` in `text`, as in
/// `buildTypes { … }`.
fn block<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.match_indices(name).find_map(|(at, _)| {
        let word_before = text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        let rest = text[at + name.len()..].trim_start();
        if word_before || !rest.starts_with('{') {
            return None;
        }
        let body = &rest[1..];
        Some(&body[..closing(body)?])
    })
}

/// Where the brace that closes a block is, in the text after the one that opened it.
fn closing(body: &str) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(at),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The blocks directly inside `body` with the name each declares, in the order written:
/// `staging { … }`, `create("staging") { … }` and `val staging by creating { … }` all name
/// `staging`.
fn entries(body: &str) -> Vec<(String, &str)> {
    let mut found = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('{') {
        let header = rest[..open].lines().next_back().unwrap_or_default().trim();
        let inner = &rest[open + 1..];
        let Some(end) = closing(inner) else { break };
        if let Some(name) = entry_name(header) {
            found.push((name, &inner[..end]));
        }
        rest = &inner[end + 1..];
    }
    found
}

/// The name a block's header declares. None for one that configures every entry.
fn entry_name(header: &str) -> Option<String> {
    let name = if let Some(quoted) = header.split(['"', '\'']).nth(1) {
        quoted
    } else {
        let mut words = header.split_whitespace();
        match words.next()? {
            // Kotlin's `val staging by creating`.
            "val" => words.next()?,
            word => word,
        }
    };
    let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    (plain && !NOT_ENTRIES.contains(&name)).then(|| name.to_string())
}

/// The build types of `build`: `debug` first and `release` last, which every project has, with
/// the ones the file adds between them.
fn build_types(build: &str) -> Vec<String> {
    let mut types = vec!["debug".to_string()];
    for (name, _) in block(build, "buildTypes").map(entries).unwrap_or_default() {
        if name != "release" && !types.contains(&name) {
            types.push(name);
        }
    }
    types.push("release".to_string());
    types
}

/// Each way of picking one flavor from every dimension of `build`, in the order Gradle names
/// them. One empty pick when the project has no flavors.
fn flavor_picks(build: &str) -> Vec<Vec<String>> {
    let flavors: Vec<(String, Option<String>)> = block(build, "productFlavors")
        .map(entries)
        .unwrap_or_default()
        .into_iter()
        .map(|(name, body)| (name, quoted_after(body, "dimension")))
        .collect();
    if flavors.is_empty() {
        return vec![Vec::new()];
    }
    let mut dimensions: Vec<String> = build
        .lines()
        .filter(|line| line.trim_start().starts_with("flavorDimensions"))
        .flat_map(|line| line.split(['"', '\'']).skip(1).step_by(2))
        .map(str::to_string)
        .collect();
    if dimensions.is_empty() {
        // One dimension needs no name on its flavors.
        dimensions.push(String::new());
    }
    let only = dimensions.len() == 1;
    let mut picks: Vec<Vec<String>> = vec![Vec::new()];
    for dimension in &dimensions {
        let names: Vec<&String> = flavors
            .iter()
            .filter(|(_, of)| only || of.as_ref() == Some(dimension))
            .map(|(name, _)| name)
            .collect();
        if names.is_empty() {
            continue;
        }
        picks = picks
            .iter()
            .flat_map(|pick| {
                names.iter().map(move |name| {
                    let mut pick = pick.clone();
                    pick.push((*name).clone());
                    pick
                })
            })
            .collect();
    }
    picks
}

/// The variants `project` can be built as, the one a plain run builds first.
///
/// A Flutter app is built in Flutter's own three modes, whatever build types its Gradle file
/// has. Reads the app module's build file, so call it off the UI thread.
#[must_use]
pub fn variants(project: &MobileProject) -> Vec<Variant> {
    let build = build_file(project).unwrap_or_default();
    let types = if project.framework == Framework::Flutter {
        ["debug", "profile", "release"].map(str::to_string).to_vec()
    } else {
        build_types(&build)
    };
    let picks = flavor_picks(&build);
    let mut variants: Vec<Variant> = types
        .iter()
        .flat_map(|build_type| {
            picks.iter().map(|flavors| Variant {
                flavors: flavors.clone(),
                build_type: build_type.clone(),
            })
        })
        .collect();
    variants.truncate(VARIANT_LIMIT);
    variants
}

/// The id `variant` of the app is installed under, read from the app's module.
///
/// It is the `applicationId` of a flavor of the variant, else of the build file, else the file's
/// `namespace`, else the `package` of the manifest, which is where builds older than the
/// namespace kept it; then each `applicationIdSuffix` the variant's flavors and build type set.
/// An Expo app that has no `android` folder yet names its id in `app.json`.
pub(super) fn application_id(project: &MobileProject, variant: &Variant) -> Option<String> {
    let Some(build) = build_file(project) else {
        return expo_package(project);
    };
    let folder = app_folder(project);
    let flavors: Vec<&str> = block(&build, "productFlavors")
        .map(entries)
        .unwrap_or_default()
        .into_iter()
        .filter(|(name, _)| variant.flavors.contains(name))
        .map(|(_, body)| body)
        .collect();
    let build_type = block(&build, "buildTypes")
        .map(entries)
        .unwrap_or_default()
        .into_iter()
        .find(|(name, _)| *name == variant.build_type)
        .map(|(_, body)| body);
    let id = flavors
        .iter()
        .find_map(|body| quoted_after(body, "applicationId"))
        .or_else(|| quoted_after(&build, "applicationId"))
        .or_else(|| quoted_after(&build, "namespace"))
        .or_else(|| {
            // A Kotlin Multiplatform module keeps its Android sources in `androidMain`.
            ["src/main", "src/androidMain"].iter().find_map(|sources| {
                let manifest =
                    std::fs::read_to_string(folder.join(sources).join("AndroidManifest.xml"))
                        .ok()?;
                quoted_after(&manifest, "package=")
            })
        })?;
    let suffixes: String = flavors
        .iter()
        .copied()
        .chain(build_type)
        .filter_map(|body| quoted_after(body, "applicationIdSuffix"))
        .collect();
    package_name(format!("{id}{suffixes}"))
}

/// The Android package an Expo app names in its `app.json`.
fn expo_package(project: &MobileProject) -> Option<String> {
    if project.framework != Framework::Expo {
        return None;
    }
    let text = std::fs::read_to_string(project.root.join("app.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    package_name(json["expo"]["android"]["package"].as_str()?.to_string())
}

/// `id` when it holds only what a package name is made of, so it can go on a command line.
fn package_name(id: String) -> Option<String> {
    let plain = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_'));
    plain.then_some(id)
}

impl Server {
    /// The variant last picked for a project, when the project still has it; else the one a
    /// plain run builds. Reads the app's build file, so call it off the UI thread.
    #[must_use]
    pub fn android_variant(&self, project_id: &str, project: &MobileProject) -> Variant {
        let picked = self.store.state(&format!("{VARIANT_KEY}{project_id}"));
        let all = variants(project);
        all.iter()
            .find(|variant| Some(variant.name()) == picked)
            .or_else(|| all.first())
            .cloned()
            .unwrap_or_default()
    }

    /// Remembers the variant picked for a project.
    ///
    /// # Errors
    ///
    /// Returns an error when the app's state cannot be saved.
    pub fn set_android_variant(&self, project_id: &str, variant: &Variant) -> Result<(), Error> {
        self.store
            .set_state(&format!("{VARIANT_KEY}{project_id}"), &variant.name())
    }
}
