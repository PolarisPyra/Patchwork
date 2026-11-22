//! Whether a project is a mobile app, and which framework builds it.
//!
//! Each framework is one function in [`DETECTORS`] that looks at a single folder; supporting
//! another framework is writing its function and adding it to the list. The project's own folder is
//! looked at first, then the folders inside it and the ones inside those, so an app kept in a
//! monorepo's `apps/mobile` is found too.

// Rust guideline compliant 2026-10-09

use std::path::{Path, PathBuf};

use super::types::{Framework, MobileProject};

/// One framework's look at `folder`, which is inside `project` or is it.
type Detector = fn(folder: &Path, project: &Path) -> Option<MobileProject>;

/// The frameworks, the most specific first: a Flutter or React Native app has a Gradle project
/// in its `android` folder, and a Kotlin Multiplatform app applies the Android plugin too.
const DETECTORS: [Detector; 4] = [flutter, react_native, kotlin_multiplatform, native_android];

/// How far below the project's folder an app is looked for: `apps/mobile` in a monorepo.
const DEPTH: usize = 2;

/// The most folders looked at on one level, so a project with thousands of folders stays quick.
const LEVEL_LIMIT: usize = 200;

/// Folders that hold dependencies and build output, never the app itself.
const SKIPPED: [&str; 8] = [
    "node_modules",
    "build",
    "target",
    "dist",
    "out",
    "Pods",
    "vendor",
    "third_party",
];

/// The files whose change may make a project a mobile app, or stop it being one.
const MARKERS: [&str; 10] = [
    "pubspec.yaml",
    "package.json",
    "app.json",
    "app.config.js",
    "app.config.ts",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "local.properties",
];

/// The mobile app in the project at `root`, or None when it has none.
///
/// Reads a handful of small files, so call it off the UI thread.
#[must_use]
pub fn detect(root: &Path) -> Option<MobileProject> {
    let mut level = vec![root.to_path_buf()];
    for depth in 0..=DEPTH {
        let found = level
            .iter()
            .find_map(|folder| DETECTORS.iter().find_map(|detect| detect(folder, root)));
        if found.is_some() || depth == DEPTH {
            return found;
        }
        level = level
            .iter()
            .flat_map(|folder| subfolders(folder))
            .take(LEVEL_LIMIT)
            .collect();
    }
    None
}

/// Whether a change to the project-relative `path` may change what [`detect`] finds.
#[must_use]
pub fn is_marker(path: &str) -> bool {
    let mut parts = path.rsplit('/');
    let name = parts.next().unwrap_or_default();
    MARKERS.contains(&name) || name == "android"
}

/// The folders inside `folder` an app may be in, in name order.
fn subfolders(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && !SKIPPED.contains(&name.as_ref())
        })
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    found.sort();
    found
}

fn read(folder: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(folder.join(name)).ok()
}

/// A `pubspec.yaml` that depends on the Flutter SDK, beside an `android` folder.
fn flutter(folder: &Path, _: &Path) -> Option<MobileProject> {
    let pubspec = read(folder, "pubspec.yaml")?;
    (depends_on_flutter(&pubspec) && folder.join("android").is_dir()).then(|| MobileProject {
        framework: Framework::Flutter,
        root: folder.to_path_buf(),
        module: None,
    })
}

/// Whether a pubspec's `dependencies` name the Flutter SDK: `flutter:` with `sdk: flutter` under
/// it, or the two on one line.
fn depends_on_flutter(pubspec: &str) -> bool {
    let mut in_dependencies = false;
    let mut after_flutter = false;
    for line in pubspec.lines() {
        let text = line.split('#').next().unwrap_or_default().trim_end();
        if text.trim().is_empty() {
            continue;
        }
        if !text.starts_with([' ', '\t']) {
            in_dependencies = text.trim() == "dependencies:";
            after_flutter = false;
            continue;
        }
        if !in_dependencies {
            continue;
        }
        let entry = text.trim();
        let names_sdk = |value: &str| value.contains("sdk:") && value.contains("flutter");
        if after_flutter && names_sdk(entry) {
            return true;
        }
        after_flutter = entry == "flutter:";
        if let Some(inline) = entry.strip_prefix("flutter:")
            && names_sdk(inline)
        {
            return true;
        }
    }
    false
}

/// A `package.json` that depends on `expo` or `react-native`, beside an `android` folder or an
/// Expo config, which makes that folder when the app is first built.
fn react_native(folder: &Path, _: &Path) -> Option<MobileProject> {
    let manifest: serde_json::Value = serde_json::from_str(&read(folder, "package.json")?).ok()?;
    let depends = |name: &str| {
        ["dependencies", "devDependencies"]
            .iter()
            .any(|section| manifest.get(section).and_then(|s| s.get(name)).is_some())
    };
    let framework = if depends("expo") {
        Framework::Expo
    } else if depends("react-native") {
        Framework::ReactNative
    } else {
        return None;
    };
    let expo_config = [
        "app.config.js",
        "app.config.ts",
        "app.config.mjs",
        "app.config.cjs",
    ]
    .iter()
    .any(|name| folder.join(name).is_file())
        || read(folder, "app.json")
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|config| config.get("expo").is_some());
    (folder.join("android").is_dir() || expo_config).then(|| MobileProject {
        framework,
        root: folder.to_path_buf(),
        module: None,
    })
}

/// A Gradle build that applies the Kotlin Multiplatform plugin and has an Android target.
fn kotlin_multiplatform(folder: &Path, project: &Path) -> Option<MobileProject> {
    let build = GradleBuild::read(folder)?;
    let texts = || build.texts();
    let multiplatform = texts().any(|text| applies(text, "kotlinmultiplatform"));
    let android = texts().any(|text| {
        text.contains("androidTarget")
            || applies(text, "androidapplication")
            || applies(text, "androidlibrary")
            || applies(text, "androidkotlinmultiplatformlibrary")
    });
    (multiplatform && android).then(|| build.project(Framework::KotlinMultiplatform, project))
}

/// A Gradle build that applies `com.android.application`.
fn native_android(folder: &Path, project: &Path) -> Option<MobileProject> {
    let build = GradleBuild::read(folder)?;
    build
        .texts()
        .any(|text| applies(text, "androidapplication"))
        .then(|| build.project(Framework::NativeAndroid, project))
}

/// The build files of a folder and of the modules directly inside it.
struct GradleBuild {
    folder: PathBuf,
    /// The folder's own `build.gradle(.kts)` and `settings.gradle(.kts)`.
    own: Vec<String>,
    /// Each module's name and its `build.gradle(.kts)`.
    modules: Vec<(String, String)>,
}

impl GradleBuild {
    /// None when `folder` has no Gradle files of its own.
    fn read(folder: &Path) -> Option<Self> {
        let own: Vec<String> = [
            "build.gradle.kts",
            "build.gradle",
            "settings.gradle.kts",
            "settings.gradle",
        ]
        .iter()
        .filter_map(|name| read(folder, name))
        .collect();
        if own.is_empty() {
            return None;
        }
        let modules = subfolders(folder)
            .into_iter()
            .filter_map(|module| {
                let text =
                    read(&module, "build.gradle.kts").or_else(|| read(&module, "build.gradle"))?;
                Some((module.file_name()?.to_string_lossy().into_owned(), text))
            })
            .collect();
        Some(Self {
            folder: folder.to_path_buf(),
            own,
            modules,
        })
    }

    fn texts(&self) -> impl Iterator<Item = &str> {
        self.own
            .iter()
            .map(String::as_str)
            .chain(self.modules.iter().map(|(_, text)| text.as_str()))
    }

    /// The project this build is: run from the folder `gradlew` is in, which is the nearest one
    /// at or above this folder with a settings file, on the module that is the app.
    fn project(&self, framework: Framework, project: &Path) -> MobileProject {
        let app = self
            .modules
            .iter()
            .find(|(_, text)| applies(text, "androidapplication"))
            .map(|(name, _)| name.clone());
        let is_root = |folder: &Path| {
            folder.join("settings.gradle.kts").is_file() || folder.join("settings.gradle").is_file()
        };
        let root = self
            .folder
            .ancestors()
            .take_while(|folder| folder.starts_with(project))
            .find(|folder| is_root(folder))
            .unwrap_or(&self.folder);
        // The folders from the Gradle root down to this one are modules too, as `:apps:android`.
        let mut path: Vec<String> = self
            .folder
            .strip_prefix(root)
            .map(|rest| {
                rest.components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        path.extend(app);
        MobileProject {
            framework,
            root: root.to_path_buf(),
            module: (!path.is_empty()).then(|| path.join(":")),
        }
    }
}

/// Whether a build file applies the plugin whose id, accessor or version-catalog alias comes to
/// `plugin` once lowercased and stripped of punctuation: `com.android.application`,
/// `libs.plugins.androidApplication` and `libs.plugins.android.application` are all
/// `androidapplication`. A line that only declares the plugin's version (`apply false`) and a
/// commented-out one do not count.
fn applies(build_file: &str, plugin: &str) -> bool {
    build_file.lines().any(|line| {
        let line = line.trim();
        if line.starts_with("//") || line.starts_with('*') || line.starts_with("/*") {
            return false;
        }
        let plain: String = line
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        plain.contains(plugin) && !plain.contains("applyfalse")
    })
}
