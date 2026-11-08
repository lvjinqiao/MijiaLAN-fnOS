#!/usr/bin/env python3
"""Source-bound package manifests shared by CI and the signing-only release job."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def verify(folder, source, target, repository):
    info = json.loads((folder / 'build-info.json').read_text(encoding='utf-8-sig'))
    if (info['source_sha'], info['target'], info['repository'], info['profile']) != (source, target, repository, 'release'):
        raise ValueError('CI package source, target, repository or profile does not match the release')
    if info['tests_passed'] <= 0 or info['checks'] != ['fmt', 'clippy', 'test', 'cli-help', 'cli-demo']:
        raise ValueError('CI package has no complete validation record')
    for name, expected in info['files'].items():
        if Path(name).name != name or not (folder / name).is_file() or digest(folder / name) != expected:
            raise ValueError('CI package checksum or filename is invalid')
    archives = [folder / name for name in info['files'] if name.endswith('.zip')]
    if len(archives) != 1:
        raise ValueError('Expected exactly one CI package')
    return info, archives[0]


def record(args):
    log = re.sub(r'\x1b\[[0-9;]*m', '', args.tests.read_text(encoding='utf-8-sig'))
    if 'test result: FAILED' in log:
        raise ValueError('Tests failed')
    count = sum(map(int, re.findall(r'test result: ok\. (\d+) passed', log)))
    info = {
        'source_sha': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
        'repository': os.environ['GITHUB_REPOSITORY'], 'target': args.target,
        'profile': 'release', 'tests_passed': count,
        'checks': ['fmt', 'clippy', 'test', 'cli-help', 'cli-demo'],
        'files': {p.name: digest(p) for p in sorted(args.folder.iterdir()) if p.suffix in ['.zip', '.sha256']},
    }
    if count <= 0 or not any(n.endswith('.zip') for n in info['files']):
        raise ValueError('Missing tests or package')
    (args.folder / 'build-info.json').write_text(json.dumps(info, indent=2) + '\n')
    Path('target/ci-source.json').write_text(json.dumps({'program': program_hash()}))
    summary = os.environ.get('GITHUB_STEP_SUMMARY')
    if summary:
        with open(summary, 'a') as f:
            f.write(f"\nTarget: `{args.target}` · profile: `release` · {count} tests passed\n\n")
            f.write(f"Rust cache exact hit: `{os.environ.get('RUST_CACHE_HIT', 'false') or 'false'}`\n")
    print(json.dumps(info, indent=2))


def prepare_mac(args):
    _, archive = verify(args.folder, args.source, 'aarch64-apple-darwin', os.environ['GITHUB_REPOSITORY'])
    version = tomllib.loads(Path('Cargo.toml').read_text())['workspace']['package']['version']
    release = Path(f'build/releases/v{version}-rs.{args.build}').resolve()
    stage = release / 'staging'
    stage.mkdir(parents=True, exist_ok=True)
    subprocess.run(['ditto', '-x', '-k', str(archive), str(stage)], check=True)
    app = stage / 'MijiaLAN.app'
    import plistlib
    plist = app / 'Contents/Info.plist'
    info = plistlib.loads(plist.read_bytes())
    if info['CFBundleIdentifier'] != 'xyz.ipocket.MijiaLAN.Rust' or info['CFBundleShortVersionString'] != version:
        raise ValueError('Unexpected application identity or version')
    for name in ['mijialan-rust', 'mijia-rust']:
        binary = app / 'Contents/MacOS' / name
        subprocess.run(['lipo', str(binary), '-verify_arch', 'arm64'], check=True)
        subprocess.run(['codesign', '--force', '--sign', os.environ['MIJIALAN_SIGNING_IDENTITY'],
                        '--options', 'runtime', '--timestamp', str(binary)], check=True)
    info.update(CFBundleName='MijiaLAN', CFBundleDisplayName='MijiaLAN', CFBundleVersion=str(args.build))
    plist.write_bytes(plistlib.dumps(info))
    print('Prepared verified CI payload for signing; no Rust compilation required')


def program_hash():
    paths = ['crates', 'vendor', 'resources', 'Cargo.toml', 'Cargo.lock']
    paths += [p for p in ['.cargo/config', '.cargo/config.toml'] if Path(p).exists()]
    identities = [subprocess.check_output(['git', 'rev-parse', 'HEAD:' + p], text=True).strip() for p in paths]
    return hashlib.sha256('\n'.join(identities).encode()).hexdigest()


def normalize(paths):
    directories = set()
    for name in paths:
        p = Path(name)
        if p.is_file() and not p.is_symlink():
            os.utime(p, (946684800, 946684800))
            directories.update(parent for parent in p.parents if parent != Path('.'))
    for p in sorted(directories, key=lambda p: len(p.parts), reverse=True):
        if p.is_dir() and not p.is_symlink():
            os.utime(p, (946684800, 946684800))


def tracked(*paths):
    return filter(None, subprocess.check_output(['git', 'ls-files', '-z', '--', *paths], encoding='utf-8').split('\0'))


def vendor():
    tree = subprocess.check_output(['git', 'rev-parse', 'HEAD:vendor'], text=True).strip()
    normalize(tracked('vendor'))
    with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
        f.write(f'tree={tree}\n')


def cache_keys(args):
    # Compiler/SDK/profile are isolated even for a dependency-only fallback.
    prefixes = ('CARGO', 'CC', 'CFLAGS', 'CXX', 'CMAKE', 'RUST', 'MACOSX_DEPLOYMENT_TARGET',
                'DEVELOPER_DIR', 'VCToolsVersion', 'WindowsSDKVersion', 'GPUI_FXC_PATH')
    environment = '\n'.join(f'{k}={v}' for k, v in sorted(os.environ.items()) if k.startswith(prefixes))
    config = hashlib.sha256(environment.encode()).hexdigest()[:20]
    vendor_tree = subprocess.check_output(['git', 'rev-parse', 'HEAD:vendor'], text=True).strip()
    prefix = f'release-v4-{os.environ["RUNNER_OS"]}-{os.environ["RUNNER_ARCH"]}-{args.target}-{args.compiler}-{config}-{vendor_tree}-'
    with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
        f.write(f'prefix={prefix}\nkey={prefix}{program_hash()}\n')


def freshness():
    marker = Path('target/ci-source.json')
    cached = json.loads(marker.read_text()) if marker.exists() else {}
    if cached.get('program') == program_hash():
        normalize(tracked('crates', 'vendor', 'resources', 'Cargo.toml', 'Cargo.lock', '.cargo'))
        print('Exact compiler-input content match; preserving Cargo freshness for unchanged sources')
    else:
        print('Compiler inputs changed or no source marker; normal Cargo rebuilds remain enabled')


def matrix(platform):
    rows = [
        dict(os='windows-2022', platform='Windows', arch='x86_64', target='x86_64-pc-windows-msvc', shell='pwsh', python='python', jobs=2),
        dict(os='macos-26', platform='macOS', arch='arm64', target='aarch64-apple-darwin', shell='bash', python='python3', jobs=1),
        dict(os='macos-26-intel', platform='macOS', arch='x86_64', target='x86_64-apple-darwin', shell='bash', python='python3', jobs=2),
    ]
    selected = {'all': rows, 'release': rows[:2], 'windows': rows[:1], 'macos-arm64': rows[1:2], 'macos-intel': rows[2:]}[platform]
    with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
        f.write('matrix=' + json.dumps({'include': selected}) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('record'); p.add_argument('--folder', type=Path, default=Path('artifacts')); p.add_argument('--target', required=True); p.add_argument('--tests', type=Path, required=True)
    p = sub.add_parser('prepare-mac'); p.add_argument('--folder', type=Path, required=True); p.add_argument('--source', required=True); p.add_argument('--build', type=int, required=True)
    sub.add_parser('vendor')
    p = sub.add_parser('cache-keys'); p.add_argument('--target', required=True); p.add_argument('--compiler', required=True)
    sub.add_parser('freshness')
    p = sub.add_parser('matrix'); p.add_argument('platform', choices=['all', 'release', 'windows', 'macos-arm64', 'macos-intel'])
    args = parser.parse_args()
    {'record': lambda: record(args), 'prepare-mac': lambda: prepare_mac(args), 'vendor': vendor, 'cache-keys': lambda: cache_keys(args), 'freshness': freshness, 'matrix': lambda: matrix(args.platform)}[args.command]()
