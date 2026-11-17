//! Project detection against small projects written to disk, and SDK discovery against each
//! platform's conventions, which run the same on any of them.

// Rust guideline compliant 2026-10-09

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::detect::{detect, is_marker};
use super::sdk::{Host, Os, Tool, abi_for, avd_home, candidates, default_root, sdk_dir, tool_path};
use super::types::{Framework, SdkSource};

/// A project folder under the repository's git-ignored `research/`, removed when dropped.
struct Fixture(PathBuf);

impl Fixture {
    /// Writes `files`, each a path relative to the project and its text. A path ending in `/`
    /// is a folder.
    fn new(files: &[(&str, &str)]) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../research/android-tests")
            .join(crate::system::new_id());
        for (path, text) in files {
            let full = root.join(path);
            if path.ends_with('/') {
                std::fs::create_dir_all(&full).unwrap();
            } else {
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, text).unwrap();
            }
        }
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const FLUTTER_PUBSPEC: &str =
    "name: shop\ndependencies:\n  flutter:\n    sdk: flutter\n  http: ^1.0.0\n";
const SETTINGS: &str = "rootProject.name = \"shop\"\ninclude(\":app\")\n";
const ROOT_BUILD: &str = "plugins {\n    alias(libs.plugins.android.application) apply false\n    alias(libs.plugins.kotlinMultiplatform) apply false\n}\n";

#[test]
fn a_flutter_app_needs_the_flutter_sdk_and_an_android_folder() {
    let app = Fixture::new(&[("pubspec.yaml", FLUTTER_PUBSPEC), ("android/", "")]);
    let found = detect(&app.0).expect("a Flutter app");
    assert_eq!(found.framework, Framework::Flutter);
    assert_eq!(found.root, app.0);

    let no_android = Fixture::new(&[("pubspec.yaml", FLUTTER_PUBSPEC)]);
    assert_eq!(detect(&no_android.0), None);

    // A Dart package names Flutter only under its dev dependencies, or not at all.
    let dart = Fixture::new(&[
        (
            "pubspec.yaml",
            "name: tool\ndependencies:\n  args: ^2.0.0\nflutter:\n  uses-material-design: true\n",
        ),
        ("android/", ""),
    ]);
    assert_eq!(detect(&dart.0), None);
}

#[test]
fn a_flutter_dependency_written_on_one_line_counts() {
    let app = Fixture::new(&[
        (
            "pubspec.yaml",
            "dependencies:\n  flutter: { sdk: flutter }\n",
        ),
        ("android/", ""),
    ]);
    assert_eq!(
        detect(&app.0).map(|p| p.framework),
        Some(Framework::Flutter)
    );
}

#[test]
fn react_native_needs_an_android_folder_and_expo_only_its_config() {
    let bare = Fixture::new(&[
        (
            "package.json",
            r#"{"dependencies":{"react-native":"0.76.0"}}"#,
        ),
        ("android/", ""),
    ]);
    assert_eq!(
        detect(&bare.0).map(|p| p.framework),
        Some(Framework::ReactNative)
    );

    let managed = Fixture::new(&[
        (
            "package.json",
            r#"{"dependencies":{"expo":"~52.0.0","react-native":"0.76.0"}}"#,
        ),
        ("app.json", r#"{"expo":{"name":"shop"}}"#),
    ]);
    assert_eq!(
        detect(&managed.0).map(|p| p.framework),
        Some(Framework::Expo)
    );

    let config_script = Fixture::new(&[
        ("package.json", r#"{"devDependencies":{"expo":"~52.0.0"}}"#),
        ("app.config.ts", "export default {};\n"),
    ]);
    assert_eq!(
        detect(&config_script.0).map(|p| p.framework),
        Some(Framework::Expo)
    );

    let no_native = Fixture::new(&[(
        "package.json",
        r#"{"dependencies":{"react-native":"0.76.0"}}"#,
    )]);
    assert_eq!(detect(&no_native.0), None);

    let web = Fixture::new(&[
        ("package.json", r#"{"dependencies":{"react":"19.0.0"}}"#),
        ("app.json", r#"{"name":"site"}"#),
    ]);
    assert_eq!(detect(&web.0), None);
}

#[test]
fn a_native_android_app_is_the_module_that_applies_the_plugin() {
    let app = Fixture::new(&[
        ("settings.gradle.kts", SETTINGS),
        ("build.gradle.kts", ROOT_BUILD),
        (
            "app/build.gradle.kts",
            "plugins {\n    id(\"com.android.application\")\n}\n",
        ),
    ]);
    let found = detect(&app.0).expect("an Android app");
    assert_eq!(found.framework, Framework::NativeAndroid);
    assert_eq!(found.root, app.0);
    assert_eq!(found.module.as_deref(), Some("app"));
}

#[test]
fn a_groovy_build_file_that_applies_the_plugin_itself_has_no_module() {
    let app = Fixture::new(&[("build.gradle", "apply plugin: 'com.android.application'\n")]);
    let found = detect(&app.0).expect("an Android app");
    assert_eq!(found.framework, Framework::NativeAndroid);
    assert_eq!(found.module, None);
}

#[test]
fn a_plugin_only_declared_or_commented_out_is_not_applied() {
    let declared = Fixture::new(&[
        ("settings.gradle.kts", SETTINGS),
        ("build.gradle.kts", ROOT_BUILD),
        (
            "lib/build.gradle.kts",
            "plugins {\n    id(\"com.android.library\")\n    // id(\"com.android.application\")\n}\n",
        ),
    ]);
    assert_eq!(detect(&declared.0), None);
}

#[test]
fn kotlin_multiplatform_needs_an_android_target() {
    let compose = Fixture::new(&[
        ("settings.gradle.kts", SETTINGS),
        ("build.gradle.kts", ROOT_BUILD),
        (
            "composeApp/build.gradle.kts",
            "plugins {\n    alias(libs.plugins.kotlinMultiplatform)\n    alias(libs.plugins.androidApplication)\n}\nkotlin {\n    androidTarget()\n}\n",
        ),
    ]);
    let found = detect(&compose.0).expect("a Kotlin Multiplatform app");
    assert_eq!(found.framework, Framework::KotlinMultiplatform);
    assert_eq!(found.module.as_deref(), Some("composeApp"));

    // The shared module is the multiplatform one; a separate module is the Android app.
    let split = Fixture::new(&[
        ("settings.gradle", "include ':shared', ':androidApp'\n"),
        (
            "shared/build.gradle.kts",
            "plugins {\n    kotlin(\"multiplatform\")\n    id(\"com.android.library\")\n}\n",
        ),
        (
            "androidApp/build.gradle.kts",
            "plugins {\n    id(\"com.android.application\")\n}\n",
        ),
    ]);
    let found = detect(&split.0).expect("a Kotlin Multiplatform app");
    assert_eq!(found.framework, Framework::KotlinMultiplatform);
    assert_eq!(found.module.as_deref(), Some("androidApp"));

    let desktop_only = Fixture::new(&[
        ("settings.gradle.kts", SETTINGS),
        (
            "app/build.gradle.kts",
            "plugins {\n    kotlin(\"multiplatform\")\n}\nkotlin {\n    jvm()\n}\n",
        ),
    ]);
    assert_eq!(detect(&desktop_only.0), None);
}

#[test]
fn an_app_two_folders_into_a_monorepo_is_found_and_deeper_ones_are_not() {
    let repo = Fixture::new(&[
        ("Cargo.toml", "[workspace]\n"),
        ("apps/mobile/pubspec.yaml", FLUTTER_PUBSPEC),
        ("apps/mobile/android/", ""),
        ("node_modules/react-native/android/", ""),
        (
            "node_modules/react-native/package.json",
            r#"{"dependencies":{"react-native":"0.76.0"}}"#,
        ),
    ]);
    let found = detect(&repo.0).expect("the app in apps/mobile");
    assert_eq!(found.framework, Framework::Flutter);
    assert_eq!(found.root, repo.0.join("apps").join("mobile"));

    let deep = Fixture::new(&[
        ("a/b/c/pubspec.yaml", FLUTTER_PUBSPEC),
        ("a/b/c/android/", ""),
    ]);
    assert_eq!(detect(&deep.0), None);
}

#[test]
fn a_gradle_module_below_the_settings_file_runs_from_the_gradle_root() {
    let repo = Fixture::new(&[
        ("settings.gradle.kts", "include(\":apps:android\")\n"),
        ("build.gradle.kts", ROOT_BUILD),
        (
            "apps/android/build.gradle.kts",
            "plugins {\n    id(\"com.android.application\")\n}\n",
        ),
    ]);
    let found = detect(&repo.0).expect("an Android app");
    assert_eq!(found.root, repo.0);
    assert_eq!(found.module.as_deref(), Some("apps:android"));
}

#[test]
fn a_project_that_is_not_a_mobile_app_has_none() {
    let rust = Fixture::new(&[
        ("Cargo.toml", "[package]\nname = \"x\"\n"),
        ("src/main.rs", ""),
    ]);
    assert_eq!(detect(&rust.0), None);
    assert_eq!(detect(&Fixture::new(&[]).0), None);
}

#[test]
fn the_files_that_decide_detection_are_markers() {
    for path in [
        "pubspec.yaml",
        "apps/mobile/package.json",
        "app/build.gradle.kts",
        "settings.gradle",
        "android",
        "apps/mobile/android",
    ] {
        assert!(is_marker(path), "{path}");
    }
    for path in [
        "src/main.rs",
        "lib/main.dart",
        "android/app/src/Main.kt",
        ".git/HEAD",
    ] {
        assert!(!is_marker(path), "{path}");
    }
}

/// A machine with `os`, a home folder and the variables in `vars`.
fn with_host<R>(os: Os, home: &str, vars: &[(&str, &str)], check: impl FnOnce(&Host) -> R) -> R {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect();
    let var = |name: &str| vars.get(name).cloned();
    check(&Host {
        os,
        home: Some(PathBuf::from(home)),
        var: &var,
    })
}

#[test]
fn the_sdk_is_looked_for_in_the_documented_order() {
    let vars = [
        ("ANDROID_HOME", "/opt/android-home"),
        ("ANDROID_SDK_ROOT", "/opt/android-sdk-root"),
    ];
    with_host(Os::Linux, "/home/me", &vars, |host| {
        let found = candidates(
            Some("/custom/sdk"),
            Some("sdk.dir=/from/properties\n"),
            host,
        );
        assert_eq!(
            found,
            vec![
                (SdkSource::Settings, PathBuf::from("/custom/sdk")),
                (SdkSource::AndroidHome, PathBuf::from("/opt/android-home")),
                (
                    SdkSource::AndroidSdkRoot,
                    PathBuf::from("/opt/android-sdk-root")
                ),
                (
                    SdkSource::LocalProperties,
                    PathBuf::from("/from/properties")
                ),
                (
                    SdkSource::PlatformDefault,
                    PathBuf::from("/home/me/Android/Sdk")
                ),
            ]
        );
    });
}

#[test]
fn unset_and_empty_places_are_skipped() {
    with_host(Os::Linux, "/home/me", &[("ANDROID_HOME", "  ")], |host| {
        let found = candidates(Some(""), Some("ndk.dir=/ndk\n"), host);
        assert_eq!(
            found,
            vec![(
                SdkSource::PlatformDefault,
                PathBuf::from("/home/me/Android/Sdk")
            )]
        );
    });
}

#[test]
fn a_path_typed_with_a_tilde_is_in_the_home_folder() {
    with_host(Os::Mac, "/Users/me", &[], |host| {
        let found = candidates(Some("~/sdks/android"), None, host);
        assert_eq!(
            found[0],
            (SdkSource::Settings, PathBuf::from("/Users/me/sdks/android"))
        );
    });
}

#[test]
fn each_platform_has_its_own_default_folder() {
    let windows = with_host(
        Os::Windows,
        r"C:\Users\me",
        &[("LOCALAPPDATA", r"C:\Users\me\AppData\Local")],
        default_root,
    );
    assert_eq!(
        windows,
        Some(
            PathBuf::from(r"C:\Users\me\AppData\Local")
                .join("Android")
                .join("Sdk")
        )
    );
    // Without the variable, the folder Windows keeps it in under the profile.
    let from_profile = with_host(Os::Windows, r"C:\Users\me", &[], default_root);
    assert_eq!(
        from_profile,
        Some(
            PathBuf::from(r"C:\Users\me")
                .join("AppData")
                .join("Local")
                .join("Android")
                .join("Sdk")
        )
    );
    let mac = with_host(Os::Mac, "/Users/me", &[], default_root);
    assert_eq!(mac, Some(PathBuf::from("/Users/me/Library/Android/sdk")));
    let linux = with_host(Os::Linux, "/home/me", &[], default_root);
    assert_eq!(linux, Some(PathBuf::from("/home/me/Android/Sdk")));
}

#[test]
fn sdk_dir_is_read_as_a_java_properties_value() {
    assert_eq!(
        sdk_dir(
            "## This file is generated\nsdk.dir=C\\:\\\\Users\\\\Jo Smith\\\\AppData\\\\Local\\\\Android\\\\Sdk\n"
        ),
        Some(r"C:\Users\Jo Smith\AppData\Local\Android\Sdk".to_string())
    );
    assert_eq!(
        sdk_dir("ndk.dir=/ndk\nsdk.dir = /Users/me/Library/Android/sdk\n"),
        Some("/Users/me/Library/Android/sdk".to_string())
    );
    assert_eq!(sdk_dir("flutter.sdk=/opt/flutter\n"), None);
    assert_eq!(sdk_dir("sdk.dir=\n"), None);
}

#[test]
fn windows_tools_are_exe_and_bat_files_and_spaces_in_the_path_are_kept() {
    let root = PathBuf::from(r"C:\Users\Jo Smith\AppData\Local\Android\Sdk");
    let all = |_: &Path| true;
    let find = |tool| tool_path(&root, tool, Os::Windows, &["latest".into()], &all);
    assert_eq!(
        find(Tool::Emulator),
        Some(root.join("emulator").join("emulator.exe"))
    );
    assert_eq!(
        find(Tool::Adb),
        Some(root.join("platform-tools").join("adb.exe"))
    );
    let bin = root.join("cmdline-tools").join("latest").join("bin");
    assert_eq!(find(Tool::AvdManager), Some(bin.join("avdmanager.bat")));
    assert_eq!(find(Tool::SdkManager), Some(bin.join("sdkmanager.bat")));
}

#[test]
fn mac_and_linux_tools_have_no_extension() {
    for (os, root) in [
        (Os::Mac, "/Users/me/Library/Android/sdk"),
        (Os::Linux, "/home/me/Android/Sdk"),
    ] {
        let root = PathBuf::from(root);
        let all = |_: &Path| true;
        let find = |tool| tool_path(&root, tool, os, &["latest".into()], &all);
        assert_eq!(find(Tool::Emulator), Some(root.join("emulator/emulator")));
        assert_eq!(find(Tool::Adb), Some(root.join("platform-tools/adb")));
        assert_eq!(
            find(Tool::AvdManager),
            Some(root.join("cmdline-tools/latest/bin/avdmanager"))
        );
        assert_eq!(
            find(Tool::SdkManager),
            Some(root.join("cmdline-tools/latest/bin/sdkmanager"))
        );
    }
}

#[test]
fn the_newest_command_line_tools_are_used_then_the_legacy_ones() {
    let root = PathBuf::from("/sdk");
    let versions: Vec<String> = vec!["9.0".into(), "11.0".into(), "10.0".into()];
    let all = |_: &Path| true;
    assert_eq!(
        tool_path(&root, Tool::SdkManager, Os::Linux, &versions, &|path| {
            !path.starts_with("/sdk/cmdline-tools/latest")
        }),
        Some(PathBuf::from("/sdk/cmdline-tools/11.0/bin/sdkmanager"))
    );
    assert_eq!(
        tool_path(&root, Tool::AvdManager, Os::Linux, &[], &|path| path
            .starts_with("/sdk/tools")),
        Some(PathBuf::from("/sdk/tools/bin/avdmanager"))
    );
    assert_eq!(
        tool_path(&root, Tool::SdkManager, Os::Linux, &versions, &all),
        Some(PathBuf::from("/sdk/cmdline-tools/latest/bin/sdkmanager"))
    );
    assert_eq!(
        tool_path(&root, Tool::Emulator, Os::Linux, &versions, &|_| false),
        None
    );
}

#[test]
fn virtual_devices_live_in_the_android_folder_unless_told_otherwise() {
    assert_eq!(
        with_host(Os::Linux, "/home/me", &[], avd_home),
        PathBuf::from("/home/me/.android/avd")
    );
    assert_eq!(
        with_host(Os::Windows, r"C:\Users\me", &[], avd_home),
        PathBuf::from(r"C:\Users\me").join(".android").join("avd")
    );
    assert_eq!(
        with_host(
            Os::Mac,
            "/Users/me",
            &[("ANDROID_AVD_HOME", "/Volumes/fast/avd")],
            avd_home
        ),
        PathBuf::from("/Volumes/fast/avd")
    );
    assert_eq!(
        with_host(
            Os::Linux,
            "/home/me",
            &[("ANDROID_USER_HOME", "/data/android")],
            avd_home
        ),
        PathBuf::from("/data/android/avd")
    );
}

#[test]
fn the_default_image_matches_the_machine() {
    assert_eq!(abi_for("aarch64"), "arm64-v8a");
    assert_eq!(abi_for("x86_64"), "x86_64");
    assert_eq!(abi_for("x86"), "x86_64");
}
