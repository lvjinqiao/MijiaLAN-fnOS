"""Generate a per-user MSI description from the inspected portable ZIP's files."""

import hashlib
import os
from pathlib import Path
import sys
import uuid
import xml.etree.ElementTree as ET

stage, version, output = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
namespace = uuid.uuid5(uuid.NAMESPACE_DNS, "xyz.ipocket.MijiaLAN.Rust.windows.x64")
schema = "http://schemas.microsoft.com/wix/2006/wi"
ET.register_namespace("", schema)


def child(parent, tag, **attributes):
    return ET.SubElement(parent, f"{{{schema}}}{tag}", attributes)


def stable_id(prefix, relative):
    return prefix + hashlib.sha256(relative.lower().encode()).hexdigest()[:24]


root = ET.Element(f"{{{schema}}}Wix")
product = child(
    root, "Product", Id="*", Name="MijiaLAN Rust", Language="1033",
    Version=version, Manufacturer="iPocket",
    UpgradeCode=str(namespace).upper(),
)
child(product, "Package", InstallerVersion="500", Compressed="yes",
      InstallScope="perUser",
      Description="MijiaLAN Rust desktop client")
upgrade = child(product, "Upgrade", Id=str(namespace).upper())
# Find existing test builds of the same version too. The separate detection
# rule and launch condition below prevent downgrading a newer version.
child(upgrade, "UpgradeVersion", Minimum="0.0.0", Property="PREVIOUSVERSIONS")
child(upgrade, "UpgradeVersion", Minimum=version, IncludeMinimum="no",
      OnlyDetect="yes", Property="NEWERPRODUCTFOUND")
condition = child(product, "Condition", Message="A newer version of MijiaLAN Rust is already installed.")
condition.text = "NOT NEWERPRODUCTFOUND"
child(child(product, "InstallExecuteSequence"), "RemoveExistingProducts", After="InstallInitialize")
child(product, "Media", Id="1", Cabinet="payload.cab", EmbedCab="yes")
child(product, "Property", Id="ARPNOMODIFY", Value="1")
child(product, "Property", Id="ARPNOREPAIR", Value="1")
target = child(product, "Directory", Id="TARGETDIR", Name="SourceDir")
local = child(target, "Directory", Id="LocalAppDataFolder")
programs = child(local, "Directory", Id="UserPrograms", Name="Programs")
install = child(programs, "Directory", Id="INSTALLDIR", Name="MijiaLAN Rust")
directories = {Path("."): install}
components = []
registry_key = r"Software\iPocket\MijiaLAN Rust\Installer"


def directory(relative):
    if relative not in directories:
        parent = directory(relative.parent)
        directories[relative] = child(
            parent, "Directory", Id=stable_id("D", relative.as_posix()), Name=relative.name,
        )
    return directories[relative]


cleaned_directories = set()
for path in sorted(stage.rglob("*")):
    if not path.is_file():
        continue
    relative = path.relative_to(stage)
    component_id = stable_id("C", relative.as_posix())
    component = child(
        directory(relative.parent), "Component", Id=component_id, Win64="yes",
        Guid=str(uuid.uuid5(namespace, relative.as_posix().lower())).upper(),
    )
    child(component, "File", Id=stable_id("F", relative.as_posix()),
          Name=relative.name, Source=os.path.relpath(path.resolve(), output.parent), DiskId="1", KeyPath="no")
    child(component, "RegistryValue", Root="HKCU", Key=registry_key,
          Name=component_id, Type="integer", Value="1", KeyPath="yes")
    if relative.parent not in cleaned_directories:
        child(component, "RemoveFolder", Id=stable_id("R", relative.parent.as_posix()), On="uninstall")
        cleaned_directories.add(relative.parent)
    components.append(component_id)

menu = child(child(target, "Directory", Id="ProgramMenuFolder"),
             "Directory", Id="ApplicationMenu", Name="MijiaLAN Rust")
menu_component = child(menu, "Component", Id="MenuShortcuts", Win64="yes",
                       Guid=str(uuid.uuid5(namespace, "start-menu")).upper())
child(menu_component, "Shortcut", Id="LaunchApp", Name="MijiaLAN Rust",
      Target="[INSTALLDIR]mijialan-rust.exe", WorkingDirectory="INSTALLDIR")
child(menu_component, "Shortcut", Id="LaunchDemo", Name="MijiaLAN Rust Demo",
      Target="[INSTALLDIR]mijialan-rust.exe", Arguments="--demo", WorkingDirectory="INSTALLDIR")
child(menu_component, "RegistryValue", Root="HKCU", Key=registry_key,
      Name="Shortcuts", Type="integer", Value="1", KeyPath="yes")
child(menu_component, "RemoveFolder", Id="RemoveMenu", On="uninstall")
components.append("MenuShortcuts")
feature = child(product, "Feature", Id="Application", Title="MijiaLAN Rust", Level="1")
for component_id in components:
    child(feature, "ComponentRef", Id=component_id)
ET.indent(root, space="  ")
ET.ElementTree(root).write(output, encoding="utf-8", xml_declaration=True)
