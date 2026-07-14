import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import olympus_job


class OlympusJobTests(unittest.TestCase):
    def test_build_payload_preserves_argv_without_shell_joining(self):
        payload = olympus_job.build_payload(
            node="fxbuilder",
            argv=["cargo", "test", "--workspace"],
            cwd="workspaces/olympus",
            timeout=900,
            max_output=1024,
        )
        self.assertEqual(payload["nodeId"], "fxbuilder")
        self.assertEqual(payload["argv"], ["cargo", "test", "--workspace"])
        self.assertEqual(payload["cwd"], "workspaces/olympus")
        self.assertEqual(payload["timeoutSecs"], 900)
        self.assertEqual(payload["maxOutputBytes"], 1024)
        self.assertIn("PATH", payload["envAllowlist"])
        self.assertIn("CARGO_TARGET_DIR", payload["envAllowlist"])

    def test_workspace_name_rejects_traversal_and_paths(self):
        for value in ("../escape", "nested/name", ".", "..", ""):
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    olympus_job.validate_workspace(value)

    def test_rsync_command_uses_builder_alias_and_deletes_excluded_state(self):
        with tempfile.TemporaryDirectory() as directory:
            command = olympus_job.build_rsync_command(Path(directory), "olympus")
        self.assertEqual(command[0:4], ["rsync", "-a", "--delete", "--delete-excluded"])
        self.assertIn("--exclude=.git/", command)
        self.assertIn("--exclude=node_modules/", command)
        self.assertTrue(command[-2].endswith("/"))
        self.assertEqual(
            command[-1],
            "fxbuilder:/srv/olympus/jobs/workspaces/olympus/",
        )

    def test_completed_exit_code_maps_to_process_exit(self):
        self.assertEqual(olympus_job.result_exit_code({"exitCode": 0}), 0)
        self.assertEqual(olympus_job.result_exit_code({"exitCode": 7}), 7)
        self.assertEqual(olympus_job.result_exit_code({"exitCode": None, "timedOut": True}), 124)
        self.assertEqual(olympus_job.result_exit_code({"exitCode": None, "cancelled": True}), 130)
        self.assertEqual(olympus_job.result_exit_code({"exitCode": None}), 1)


if __name__ == "__main__":
    unittest.main()
