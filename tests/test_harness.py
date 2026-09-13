"""
Harness Import & Tool Discovery Test Suite
Validates:
1. Module import safety:
   - `import run_qemu` does NOT perform import-time tool validation or call sys.exit().
   - Module import succeeds even in an isolated environment where PATH is empty or missing tools.
2. Tool resolution behavior:
   - Missing tools raise standard FileNotFoundError at call time, not at import time.
   - Backward-compatibility attributes (CARGO_BIN, NASM_BIN, etc.) resolve via lazy __getattr__.
"""

import unittest
import subprocess
import sys
import os
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
TOOLS_DIR = PROJECT_ROOT / "tools"
sys.path.insert(0, str(TOOLS_DIR))

import run_qemu


class TestHarnessImportSafety(unittest.TestCase):
    def test_import_run_qemu_in_current_process(self):
        """Verify run_qemu imports cleanly and exposes required directory/marker constants."""
        self.assertTrue(hasattr(run_qemu, "PROJECT_ROOT"))
        self.assertTrue(hasattr(run_qemu, "BUILD_DIR"))
        self.assertTrue(hasattr(run_qemu, "KERNEL_DIR"))
        self.assertTrue(hasattr(run_qemu, "BOOT_DIR"))
        self.assertTrue(hasattr(run_qemu, "EXPECTED_STAGE2_MARKERS"))
        self.assertTrue(hasattr(run_qemu, "build_stage2"))
        self.assertTrue(hasattr(run_qemu, "test_qemu"))

    def test_isolated_import_without_path(self):
        """Verify that importing run_qemu in a subprocess with stripped PATH never calls sys.exit()."""
        # Minimal environment containing only system runtime vars (to run python itself)
        minimal_env = {}
        for var in ["SYSTEMROOT", "WINDIR", "PYTHONPATH", "PYTHONHOME"]:
            if var in os.environ:
                minimal_env[var] = os.environ[var]
        minimal_env["PATH"] = ""  # Empty PATH ensures no external tools can be found via PATH

        code = (
            f"import sys\n"
            f"sys.path.insert(0, r'{TOOLS_DIR}')\n"
            f"import run_qemu\n"
            f"print('IMPORT_SUCCESS')\n"
        )
        res = subprocess.run(
            [sys.executable, "-c", code],
            env=minimal_env,
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0,
            f"import run_qemu failed in isolated environment:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}")
        self.assertIn("IMPORT_SUCCESS", res.stdout)

    def test_missing_tool_raises_filenotfound_at_call_time(self):
        """Verify that missing tools raise FileNotFoundError when invoked, not at import time."""
        minimal_env = {}
        for var in ["SYSTEMROOT", "WINDIR"]:
            if var in os.environ:
                minimal_env[var] = os.environ[var]
        minimal_env["PATH"] = ""
        # Mock USERPROFILE/HOME to empty non-existent directory so ~/.cargo / ~/.rustup are absent
        fake_home = str(PROJECT_ROOT / "build" / "nonexistent_home")
        minimal_env["USERPROFILE"] = fake_home
        minimal_env["HOME"] = fake_home

        code = (
            f"import sys\n"
            f"sys.path.insert(0, r'{TOOLS_DIR}')\n"
            f"import run_qemu\n"
            f"try:\n"
            f"    run_qemu.resolve_cargo()\n"
            f"    print('CARGO_FOUND_UNEXPECTEDLY')\n"
            f"except FileNotFoundError as e:\n"
            f"    print('EXPECTED_FILENOTFOUND:' + type(e).__name__)\n"
        )
        res = subprocess.run(
            [sys.executable, "-c", code],
            env=minimal_env,
            capture_output=True,
            text=True
        )
        self.assertEqual(res.returncode, 0,
            f"Execution failed:\nSTDOUT:\n{res.stdout}\nSTDERR:\n{res.stderr}")
        self.assertIn("EXPECTED_FILENOTFOUND:FileNotFoundError", res.stdout)

    def test_backward_compatibility_attributes(self):
        """Verify lazy __getattr__ backward compatibility for legacy *_BIN variables."""
        cargo = run_qemu.CARGO_BIN
        self.assertIsInstance(cargo, Path)
        self.assertTrue(cargo.name.startswith("cargo"))

        nasm = run_qemu.NASM_BIN
        self.assertIsInstance(nasm, Path)
        self.assertTrue(nasm.name.lower().startswith("nasm"))

        objcopy = run_qemu.OBJCOPY_BIN
        self.assertIsInstance(objcopy, Path)
        self.assertTrue("objcopy" in objcopy.name.lower())

        qemu = run_qemu.QEMU_BIN
        self.assertIsInstance(qemu, Path)
        self.assertTrue("qemu" in qemu.name.lower())

        rust_lld = run_qemu.RUST_LLD_BIN
        self.assertIsInstance(rust_lld, Path)
        self.assertTrue("rust-lld" in rust_lld.name.lower())


if __name__ == "__main__":
    unittest.main()
