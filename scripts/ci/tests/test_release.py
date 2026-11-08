import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import release


class ReleaseSourceTests(unittest.TestCase):
    def git(self, *args):
        return '[workspace.package]\nversion="0.1.0"' if args[0] == 'show' else 'a' * 40

    @patch.dict(os.environ, GITHUB_SHA='a' * 40, GITHUB_REPOSITORY='iPocket-APP/MijiaLAN')
    def test_version_and_build_come_from_exact_source(self):
        with patch.object(release, 'git', self.git), patch.object(subprocess, 'run'):
            result = release.source_plan('rs-v0.1.0-b12', '')
            self.assertEqual(result['build_number'], '12')
            self.assertEqual(result['source_sha'], 'a' * 40)
            self.assertEqual(release.source_plan('', '13')['tag'], 'rs-v0.1.0-b13')

    def test_rejects_tag_version_build_and_option_mixups(self):
        with patch.object(release, 'git', self.git), patch.object(subprocess, 'run'):
            for ref, build in [('rs-v0.2.0-b12', ''), ('rs-v0.1.0-b12', '13'), ('--upload', ''), ('rs-v0.1.0-b1２', '')]:
                with self.subTest(ref=ref), self.assertRaises(ValueError):
                    release.source_plan(ref, build)

    def test_rejects_source_outside_trusted_branch(self):
        with patch.object(release, 'git', self.git), patch.object(subprocess, 'run', side_effect=subprocess.CalledProcessError(1, 'git')):
            with self.assertRaises(subprocess.CalledProcessError):
                release.source_plan('rs-v0.1.0-b12', '')

    @patch.dict(os.environ, GITHUB_REPOSITORY='iPocket-APP/MijiaLAN')
    def test_only_trusted_same_source_build_runs_are_reused(self):
        run = dict(head_sha='a' * 40, head_branch='MijiaLAN-rs', event='push', head_repository=dict(full_name='iPocket-APP/MijiaLAN'))
        self.assertTrue(release.ready(run, 'a' * 40))
        for change in [dict(event='pull_request'), dict(head_branch='main'), dict(head_sha='b' * 40), dict(head_repository=dict(full_name='fork/MijiaLAN'))]:
            self.assertFalse(release.ready(run | change, 'a' * 40))
