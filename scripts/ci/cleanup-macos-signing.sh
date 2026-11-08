#!/usr/bin/env bash
set -euo pipefail
[[ -n "${MIJIALAN_CI_SIGNING_DIR:-}" ]] || exit 0
python3 - <<'PY'
import os, pathlib, shlex, shutil, subprocess
root = pathlib.Path(os.environ['RUNNER_TEMP']).resolve()
directory = pathlib.Path(os.environ['MIJIALAN_CI_SIGNING_DIR']).resolve()
assert directory.parent == root and directory.name.startswith('mijialan-signing.'), 'Invalid signing cleanup directory'
original = directory / 'original-keychains.txt'
keychain = directory / 'signing.keychain-db'
errors = []
try:
    if original.exists():
        subprocess.run(['security', 'list-keychains', '-d', 'user', '-s', *shlex.split(original.read_text())], check=True)
except subprocess.CalledProcessError as error:
    errors.append(str(error))
try:
    if keychain.exists():
        subprocess.run(['security', 'delete-keychain', str(keychain)], check=True)
except subprocess.CalledProcessError as error:
    errors.append(str(error))
finally:
    shutil.rmtree(directory)
if errors:
    raise SystemExit('; '.join(errors))
print('Temporary signing keychain and key files removed')
PY
