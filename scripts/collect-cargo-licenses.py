#!/usr/bin/env python3
"""Copy notices from resolved Cargo packages, without bundling source paths."""
import json
from pathlib import Path
import shutil
import sys

metadata = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8-sig"))
destination = Path(sys.argv[2])
destination.mkdir(parents=True, exist_ok=True)
rows = ["# Cargo dependency licenses", "", "| Package | SPDX expression | Notices |", "| --- | --- | --- |"]
for package in sorted(metadata["packages"], key=lambda item: (item["name"], item["version"])):
    if package["id"] in metadata["workspace_members"]:
        continue
    root = Path(package["manifest_path"]).parent
    name = f'{package["name"]}-{package["version"]}'
    notices = set()
    for path in root.iterdir():
        if path.is_file() and path.name.lower().startswith(("license", "copying", "notice", "copyright", "patents")):
            notices.add(path)
    if package.get("license_file"):
        path = root / package["license_file"]
        if path.is_file():
            notices.add(path)
    links = []
    for path in sorted(notices):
        target = destination / name / path.name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
        links.append(f"[{path.name}]({name}/{path.name})")
    rows.append(f'| {name} | {package.get("license") or "See notices"} | {", ".join(links)} |')
(destination / "README.md").write_text("\n".join(rows) + "\n", encoding="utf-8")
print(f"Collected license inventory for {len(rows) - 4} Cargo packages")
