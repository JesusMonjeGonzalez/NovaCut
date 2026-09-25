"""Validate cargo-about evidence and render deterministic Windows release notices."""

import hashlib
import html
import json
from pathlib import Path
import sys
import tomllib


def digest(data):
    return hashlib.sha256(data).hexdigest()


def generate(raw, root):
    directory = root / "docs/licenses"
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    locked = {(p["name"], p["version"]): p for p in lock["package"]}
    packages = {}
    paths = {}
    for entry in raw["crates"]:
        p = entry["package"]
        key = f'{p["name"]}@{p["version"]}'
        path = Path(p["manifest_path"]).parent
        paths[key] = path
        if path.resolve() == root.resolve():
            continue  # First-party LICENSE is shipped separately.
        if entry["license"] in ("Unknown", "Ignore", "") or not p.get("license"):
            raise ValueError(f"Missing declared license: {key}")
        record = locked[(p["name"], p["version"])]
        if not record.get("checksum") or record.get("source") != p["source"]:
            raise ValueError(f"Dependency not pinned to a registry checksum: {key}")
        packages[key] = {
            "name": p["name"], "version": p["version"],
            "source": p["source"], "checksum": record["checksum"],
            "declared_license": p["license"], "repository": p.get("repository"),
            "selected_licenses": set(), "texts": set(),
        }
    if not packages:
        raise ValueError("Empty dependency inventory")

    texts = {}

    def add(key, label, text, source):
        if not text.strip():
            raise ValueError(f"Empty license text: {key} {source}")
        sha = digest(text.encode())
        text_id = f"{label}:{sha}"
        item = texts.setdefault(text_id, {
            "license": label, "sha256": sha, "text": text, "sources": set(),
        })
        item["sources"].add(source)
        packages[key]["texts"].add(text_id)

    # Exact-version, checksum-verified upstream files, never generic SPDX text.
    upstream = {}
    for source in json.loads((directory / "upstream.json").read_text()):
        data = (directory / source["file"]).read_bytes()
        if digest(data) != source["sha256"]:
            raise ValueError(f'Changed upstream license: {source["file"]}')
        for key in source["crates"]:
            if key not in packages:
                raise ValueError(f"Stale upstream evidence: {key}")
            vcs = json.loads((paths[key] / ".cargo_vcs_info.json").read_text())
            if vcs["git"]["sha1"] != source["commit"]:
                raise ValueError(f"Upstream commit mismatch: {key}")
            upstream[key, source["license"]] = (data.decode(), source["url"])

    # These original files are missed/misclassified by cargo-about's scanner.
    local = {
        ("ab_glyph@0.2.32", "Apache-2.0"): "LICENSE",
        ("ab_glyph_rasterizer@0.1.10", "Apache-2.0"): "LICENSE",
        ("owned_ttf_parser@0.25.1", "Apache-2.0"): "LICENSE",
        ("dpi@0.1.2", "MIT"): "LICENSE-LIBM-MIT",
    }
    for key in packages:
        if packages[key]["name"].startswith(("windows-", "windows_")) or packages[key]["name"] == "windows":
            local[key, "MIT"] = "license-mit"

    for license_entry in raw["licenses"]:
        label = license_entry["id"]
        for user in license_entry["used_by"]:
            p = user["crate"]
            key = f'{p["name"]}@{p["version"]}'
            if key not in packages:
                continue
            packages[key]["selected_licenses"].add(label)
            if (key, label) in upstream:
                text, source = upstream[key, label]
            elif (key, label) in local:
                relative = local[key, label]
                text = (paths[key] / relative).read_bytes().decode("utf-8")
                source = f"{key}/{relative}"
            else:
                source_path = license_entry.get("source_path")
                if not source_path:
                    raise ValueError(f"Missing original text (SPDX fallback rejected): {key} {label}")
                path = Path(source_path)
                owners = [(k, base) for k, base in paths.items() if path.is_relative_to(base)]
                if len(owners) != 1:
                    raise ValueError(f"Untraceable license source: {source_path}")
                owner, base = owners[0]
                source = f"{owner}/{path.relative_to(base).as_posix()}"
                text = license_entry["text"]
                if text != path.read_bytes().decode("utf-8"):
                    raise ValueError(f"License differs from original file: {source}")
            add(key, label, text, source)

    # Preserve all font notices, including Hack's MIT + Bitstream Vera terms.
    fonts = "epaint_default_fonts@0.31.1"
    if fonts not in packages:
        raise ValueError("Embedded font version changed: review font evidence")
    for filename in ("Hack-Regular.txt", "OFL.txt", "UFL.txt", "emoji-icon-font-mit-license.txt"):
        relative = f"fonts/{filename}"
        add(fonts, "Font notice", (paths[fonts] / relative).read_bytes().decode("utf-8"), f"{fonts}/{relative}")

    for key, p in packages.items():
        if not p["selected_licenses"] or not p["texts"]:
            raise ValueError(f"No resolved license/text for {key}")
        # Apache and other attribution notices can be separate from LICENSE.
        for path in sorted(paths[key].rglob("*")):
            if path.is_file() and path.name.lower().split(".")[0] in ("notice", "copyright"):
                add(key, "Additional notice", path.read_bytes().decode("utf-8"), f"{key}/{path.relative_to(paths[key]).as_posix()}")
        p["selected_licenses"] = sorted(p["selected_licenses"])
        p["texts"] = sorted(p["texts"])
    for text in texts.values():
        text["sources"] = sorted(text["sources"])

    return {
        "generator": "cargo-about 0.9.2 + docs/licenses/render.py",
        "target": "x86_64-pc-windows-msvc", "features": ["default", "windows-host"],
        "scope": "Transitive normal and build dependencies; dev dependencies excluded; first-party LICENSE separate.",
        "limitations": "Cargo source inventory, not binary/linker inspection. Excludes Rust toolchain, system SDKs/runtimes, external FFmpeg/Whisper, models, user media/fonts and non-Cargo components. Other targets require a separate report.",
        "cargo_lock_sha256": digest((root / "Cargo.lock").read_bytes()),
        "cargo_toml_sha256": digest((root / "Cargo.toml").read_bytes()),
        "packages": [packages[k] for k in sorted(packages)],
        "texts": dict(sorted(texts.items())),
    }


if __name__ == "__main__":
    try:
        report = generate(json.loads(Path(sys.argv[1]).read_text()), Path(__file__).resolve().parents[2])
        output = Path(sys.argv[2])
        escape = html.escape
        parts = ['<!doctype html><html lang="en"><meta charset="utf-8">',
                 '<meta name="viewport" content="width=device-width,initial-scale=1">',
                 '<title>NovaCut Windows Third-Party Licenses</title>',
                 '<style>body{font:16px system-ui;max-width:76rem;margin:2rem auto;padding:0 1rem}pre{white-space:pre-wrap;overflow-wrap:anywhere}td,th{text-align:left;padding:.5rem;vertical-align:top}table{width:100%}a{overflow-wrap:anywhere}</style>',
                 '<h1>NovaCut Windows Third-Party Licenses</h1>',
                 f'<p>Target: {report["target"]}; features: default, windows-host. Generator: {escape(report["generator"])}.</p>',
                 f'<p>Cargo.lock SHA-256: <code>{report["cargo_lock_sha256"]}</code></p>',
                 f'<p>{escape(report["scope"])}</p><p>{escape(report["limitations"])}</p>',
                 f'<h2>Inventory ({len(report["packages"])} dependencies)</h2>',
                 '<p>Declared SPDX expressions retain alternatives; linked texts show selected obligations and additional notices.</p>',
                 '<table><tr><th>Crate</th><th>Version</th><th>Declared license</th><th>Texts</th></tr>']
        anchors = {key: f"text-{i}" for i, key in enumerate(report["texts"], 1)}
        for p in report["packages"]:
            links = ' '.join(f'<a href="#{anchors[t]}">{escape(report["texts"][t]["license"])}</a>' for t in p["texts"])
            parts.append(f'<tr><td>{escape(p["name"])}</td><td>{escape(p["version"])}</td><td>{escape(p["declared_license"])}</td><td>{links}</td></tr>')
        parts.append('</table><h2>License Texts and Notices</h2>')
        for key, text in report["texts"].items():
            users = ', '.join(f'{p["name"]} {p["version"]}' for p in report["packages"] if key in p["texts"])
            parts.extend([f'<section id="{anchors[key]}"><h3>{escape(text["license"])}</h3>',
                          f'<p>Used by: {escape(users)}</p>',
                          f'<p>Sources: {escape("; ".join(text["sources"]))}</p>',
                          f'<pre>{escape(text["text"])}</pre></section>'])
        parts.append('</html>')
        (output / "windows-inventory.json").write_text(json.dumps(report, indent=2, ensure_ascii=True) + "\n", encoding="utf-8", newline="\n")
        (output / "THIRD_PARTY_LICENSES-Windows.html").write_text("\n".join(parts) + "\n", encoding="utf-8", newline="\n")
        print(f'Validated {len(report["packages"])} dependencies and {len(report["texts"])} license/notice texts')
    except (ValueError, KeyError, OSError) as error:
        sys.exit(f"License report failed: {error}")
