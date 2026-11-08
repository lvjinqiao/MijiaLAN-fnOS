import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('build_artifact', Path(__file__).resolve().parents[1] / 'build_artifact.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PackageProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.package = self.root / 'app.zip'
        self.package.write_bytes(b'fixture package')
        self.info = dict(source_sha='a' * 40, target='aarch64-apple-darwin', repository='iPocket-APP/MijiaLAN', profile='release', tests_passed=45,
                         checks=['fmt', 'clippy', 'test', 'cli-help', 'cli-demo'], files={'app.zip': module.digest(self.package)})

    def verify(self):
        (self.root / 'build-info.json').write_text(json.dumps(self.info))
        return module.verify(self.root, 'a' * 40, 'aarch64-apple-darwin', 'iPocket-APP/MijiaLAN')

    def test_accepts_verified_package(self):
        self.assertEqual(self.verify()[1], self.package)

    def test_rejects_source_architecture_repository_and_profile_mixups(self):
        for field, value in [('source_sha', 'b' * 40), ('target', 'x86_64-apple-darwin'), ('repository', 'fork/MijiaLAN'), ('profile', 'debug')]:
            with self.subTest(field=field):
                before = self.info[field]
                self.info[field] = value
                with self.assertRaises(ValueError): self.verify()
                self.info[field] = before

    def test_rejects_tampered_bytes(self):
        self.package.write_bytes(b'changed')
        with self.assertRaises(ValueError): self.verify()

    def test_rejects_unverified_tests_and_missing_checks(self):
        self.info['tests_passed'] = 0
        with self.assertRaises(ValueError): self.verify()
        self.info['tests_passed'] = 45
        self.info['checks'].remove('clippy')
        with self.assertRaises(ValueError): self.verify()

    def test_rejects_paths_outside_artifact_and_multiple_packages(self):
        self.info['files'] = {'../app.zip': 'x'}
        with self.assertRaises(ValueError): self.verify()
        self.info['files'] = {}
        with self.assertRaises(ValueError): self.verify()


class CacheFreshnessTests(unittest.TestCase):
    def test_only_exact_compiler_content_match_restores_timestamps(self):
        import os
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            previous = Path.cwd()
            try:
                os.chdir(directory)
                Path('target').mkdir()
                Path('crates').mkdir()
                source = Path('crates/main.rs')
                source.write_text('fn main() {}')
                before = source.stat().st_mtime_ns
                with patch.object(module, 'program_hash', return_value='current'), patch.object(module, 'tracked', return_value=['crates/main.rs']):
                    Path('target/ci-source.json').write_text(json.dumps({'program': 'older'}))
                    module.freshness()
                    self.assertEqual(source.stat().st_mtime_ns, before)
                    Path('target/ci-source.json').write_text(json.dumps({'program': 'current'}))
                    module.freshness()
                    self.assertEqual(source.stat().st_mtime, 946684800)
                    self.assertEqual(Path('crates').stat().st_mtime, 946684800)
            finally:
                os.chdir(previous)
