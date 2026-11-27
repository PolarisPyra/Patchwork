#!/usr/bin/env python3
"""Vendor Zed's split GPUI crates into `vendor/` as self-contained workspace packages.

Usage (from repository root):
    uv run --no-project python3 scripts/vendor-gpui.py --zed-dir /path/to/zed-checkout

If `--zed-dir` is omitted, looks for the pinned commit in `~/.cargo/git/checkouts/zed-*`.
See `vendor/README.md` for full upgrade instructions.
"""

from __future__ import annotations

import argparse
import pathlib
import shutil
import sys
import tomllib

ZED_REV = "d2a4fb6ded7c470be25d1541281da3882495b65b"

# (source path relative to Zed root, target directory name under vendor/)
VENDOR_CRATES: list[tuple[str, str]] = [
    ("crates/collections", "collections"),
    ("crates/gpui", "gpui"),
    ("crates/gpui_apple", "gpui_apple"),
    ("crates/gpui_ios", "gpui_ios"),
    ("crates/gpui_linux", "gpui_linux"),
    ("crates/gpui_macos", "gpui_macos"),
    ("crates/gpui_macros", "gpui_macros"),
    ("crates/gpui_platform", "gpui_platform"),
    ("crates/gpui_shared_string", "gpui_shared_string"),
    ("crates/gpui_util", "gpui_util"),
    ("crates/gpui_web", "gpui_web"),
    ("crates/gpui_wgpu", "gpui_wgpu"),
    ("crates/gpui_windows", "gpui_windows"),
    ("crates/http_client", "http_client"),
    ("crates/refineable", "refineable"),
    ("crates/refineable/derive_refineable", "derive_refineable"),
    ("crates/scheduler", "scheduler"),
    ("crates/sum_tree", "sum_tree"),
    ("crates/ztracing", "ztracing"),
    ("crates/ztracing_macro", "ztracing_macro"),
]

VENDOR_NAMES = {target for _, target in VENDOR_CRATES}

# Optional Zed-internal crates we do not vendor (only used for benchmarks / tracy).
STRIPPED_OPTIONAL_DEPS = {"bench_metrics", "zlog"}

SKIP_DIRS = {"examples", "benches", "tests", "target"}


def toml_value(val: object) -> str:
    if isinstance(val, bool):
        return "true" if val else "false"
    if isinstance(val, int):
        return str(val)
    if isinstance(val, str):
        escaped = (
            val.replace("\\", "\\\\")
            .replace('"', '\\"')
            .replace("\n", "\\n")
            .replace("\t", "\\t")
        )
        return f'"{escaped}"'
    if isinstance(val, list):
        return "[" + ", ".join(toml_value(item) for item in val) + "]"
    if isinstance(val, dict):
        items = ", ".join(f"{toml_key(k)} = {toml_value(v)}" for k, v in val.items())
        return "{ " + items + " }"
    raise TypeError(f"Unsupported TOML value: {val!r}")


def toml_key(key: str) -> str:
    if key and all(c.isalnum() or c in "-_" for c in key):
        return key
    return toml_value(key)


def resolve_dep(
    name: str,
    spec: object,
    ws_deps: dict[str, object],
) -> object:
    if not isinstance(spec, dict) or not spec.get("workspace"):
        return spec

    base = ws_deps.get(name)
    if base is None:
        raise KeyError(f"Workspace dependency `{name}` not found in Zed Cargo.toml")

    merged: dict[str, object] = {}
    if isinstance(base, str):
        merged["version"] = base
    elif isinstance(base, dict):
        merged.update(base)
    else:
        raise TypeError(f"Unexpected workspace dep entry for `{name}`: {base!r}")

    # Rewrite Zed internal path deps to flat sibling vendor/ paths.
    if "path" in merged:
        dep_pkg = str(merged.get("package", name))
        if dep_pkg in VENDOR_NAMES:
            merged["path"] = f"../{dep_pkg}"
            merged.pop("version", None)
        else:
            raise KeyError(f"Unvendored Zed internal dependency `{name}` ({merged['path']})")

    local_features = spec.get("features")
    if local_features:
        base_features = list(merged.get("features", []))
        for feat in local_features:
            if feat not in base_features:
                base_features.append(feat)
        merged["features"] = base_features

    for key in ("optional", "default-features", "package"):
        if key in spec:
            merged[key] = spec[key]

    return merged


def filter_deps(
    deps: dict[str, object],
    ws_deps: dict[str, object],
) -> dict[str, object]:
    out: dict[str, object] = {}
    for name, spec in deps.items():
        if name in STRIPPED_OPTIONAL_DEPS:
            continue
        out[name] = resolve_dep(name, spec, ws_deps)
    return out


def filter_features(features: dict[str, list[str]]) -> dict[str, list[str]]:
    out: dict[str, list[str]] = {}
    for feat, items in features.items():
        cleaned = [
            item
            for item in items
            if not any(
                item == f"dep:{stripped}"
                or item.startswith(f"{stripped}/")
                or item.startswith(f"{stripped}?/")
                for stripped in STRIPPED_OPTIONAL_DEPS
            )
        ]
        out[feat] = cleaned
    return out


def emit_table(lines: list[str], header: str, mapping: dict[str, object]) -> None:
    if not mapping:
        return
    lines.append(f"[{header}]")
    for k, v in mapping.items():
        lines.append(f"{toml_key(k)} = {toml_value(v)}")
    lines.append("")


def rewrite_manifest(
    src_toml: pathlib.Path,
    ws_pkg: dict[str, object],
    ws_deps: dict[str, object],
) -> str:
    data = tomllib.loads(src_toml.read_text())
    lines: list[str] = [
        f"# Vendored from https://github.com/zed-industries/zed @ {ZED_REV}",
        "# Regenerate via: uv run --no-project python3 scripts/vendor-gpui.py",
        "",
    ]

    pkg = dict(data["package"])
    for k, v in list(pkg.items()):
        if isinstance(v, dict) and v.get("workspace"):
            if k in ws_pkg:
                pkg[k] = ws_pkg[k]
            else:
                del pkg[k]
    pkg["publish"] = False
    # Never inherit Patchwork's workspace lints; silence upstream check-cfg and clippy noise.
    emit_table(lines, "package", pkg)
    emit_table(lines, "lints.rust", {"unexpected_cfgs": "allow"})
    emit_table(lines, "lints.clippy", {"all": {"level": "allow", "priority": -1}})

    lib_table = dict(data.get("lib", {}))
    lib_table["test"] = False
    lib_table["doctest"] = False
    emit_table(lines, "lib", lib_table)

    if "features" in data:
        emit_table(lines, "features", filter_features(data["features"]))

    if "dependencies" in data:
        emit_table(lines, "dependencies", filter_deps(data["dependencies"], ws_deps))

    if "build-dependencies" in data:
        emit_table(
            lines,
            "build-dependencies",
            filter_deps(data["build-dependencies"], ws_deps),
        )

    for cfg_key, cfg_val in data.get("target", {}).items():
        if not isinstance(cfg_val, dict):
            continue
        if "dependencies" in cfg_val:
            emit_table(
                lines,
                f"target.{toml_key(cfg_key)}.dependencies",
                filter_deps(cfg_val["dependencies"], ws_deps),
            )
        if "build-dependencies" in cfg_val:
            emit_table(
                lines,
                f"target.{toml_key(cfg_key)}.build-dependencies",
                filter_deps(cfg_val["build-dependencies"], ws_deps),
            )

    return "\n".join(lines).rstrip() + "\n"


def find_default_zed_dir() -> pathlib.Path | None:
    short = ZED_REV[:7]
    checkouts = pathlib.Path.home() / ".cargo" / "git" / "checkouts"
    if not checkouts.is_dir():
        return None
    for zed_checkout in sorted(checkouts.glob("zed-*")):
        candidate = zed_checkout / short
        if (candidate / "Cargo.toml").is_file():
            return candidate
    return None


def copy_crate(src: pathlib.Path, dst: pathlib.Path) -> None:
    if dst.exists():
        shutil.rmtree(dst)

    def ignore(_dir: str, entries: list[str]) -> set[str]:
        ignored: set[str] = set()
        for entry in entries:
            if entry in SKIP_DIRS or entry == "derive_refineable":
                ignored.add(entry)
        return ignored

    shutil.copytree(src, dst, ignore=ignore)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--zed-dir",
        type=pathlib.Path,
        default=None,
        help="Path to a checkout of zed-industries/zed",
    )
    parser.add_argument(
        "--vendor-dir",
        type=pathlib.Path,
        default=pathlib.Path("vendor"),
        help="Destination vendor directory (default: vendor)",
    )
    args = parser.parse_args()

    zed_dir = args.zed_dir or find_default_zed_dir()
    if zed_dir is None or not (zed_dir / "Cargo.toml").is_file():
        print(
            f"error: could not locate zed checkout for {ZED_REV}; pass --zed-dir /path/to/zed",
            file=sys.stderr,
        )
        return 1

    root_toml = tomllib.loads((zed_dir / "Cargo.toml").read_text())
    ws_pkg = root_toml.get("workspace", {}).get("package", {})
    ws_deps = root_toml.get("workspace", {}).get("dependencies", {})

    args.vendor_dir.mkdir(parents=True, exist_ok=True)
    for rel_src, dst_name in VENDOR_CRATES:
        src = zed_dir / rel_src
        dst = args.vendor_dir / dst_name
        copy_crate(src, dst)
        rewritten = rewrite_manifest(src / "Cargo.toml", ws_pkg, ws_deps)
        (dst / "Cargo.toml").write_text(rewritten)
        print(f"vendored {rel_src} -> {dst}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
