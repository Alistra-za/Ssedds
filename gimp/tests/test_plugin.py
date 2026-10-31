# SPDX-License-Identifier: GPL-3.0-or-later
"""tessalith-dds.py loaded with gi stubbed: what the procedures do when psdds.dll fails (Note FC6.5)."""
import contextlib
import importlib.util
import io
import os
import sys
import unittest
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
PLUG = os.path.join(REPO, "gimp", "tessalith-dds")
sys.path.insert(0, PLUG)
os.environ.setdefault("TESSALITH_PSDDS_DLL", os.path.join(
    REPO, "target", "psdds", "x86_64-pc-windows-msvc", "release", "psdds.dll"))

import psdds_ctypes as P  # noqa: E402

MISSING = os.path.join(HERE, "no-such-dir", "psdds.dll")


class _PlugIn:
    __gtype__ = None

    @staticmethod
    def error_quark():
        return 0


def load_plugin():
    gimp = mock.MagicMock()
    gimp.PlugIn = _PlugIn
    repo = mock.MagicMock(Gimp=gimp)
    with mock.patch.dict(sys.modules, {"gi": mock.MagicMock(repository=repo), "gi.repository": repo}):
        spec = importlib.util.spec_from_file_location("tessalith_dds_plugin", os.path.join(PLUG, "tessalith-dds.py"))
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
    return mod, gimp, repo.GLib


class PluginWithoutDll(unittest.TestCase):
    def setUp(self):
        self.mod, self.gimp, self.glib = load_plugin()
        with self.assertRaises(P.PsddsError) as c:
            P.Psdds(MISSING)
        self.message = str(c.exception)
        self.assertIn(MISSING, self.message)
        self.assertIn(f"ABI version {P.EXPECTED_ABI}", self.message)

    def returned_error(self, procedure):
        procedure.new_return_values.assert_called_once()
        self.assertEqual(procedure.new_return_values.call_args[0][0], self.gimp.PDBStatusType.EXECUTION_ERROR)
        return self.glib.Error.new_literal.call_args[0][1]

    def test_both_procedures_register_and_return_the_message(self):
        with mock.patch.dict(os.environ, {"TESSALITH_PSDDS_DLL": MISSING}):
            plugin = self.mod.TessalithDds()
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                names = plugin.do_query_procedures()
            self.assertEqual(names, [self.mod.LOAD_PROC, self.mod.EXPORT_PROC])
            self.assertIn(self.message, err.getvalue())
            plugin._export_procedure(self.mod.EXPORT_PROC)  # registers with the fixed defaults

            procedure, image, file, options, img = (mock.MagicMock() for _ in range(5))
            file.get_path.return_value = os.path.join(HERE, "out.dds")
            image.get_layers.return_value = [mock.MagicMock(**{"get_visible.return_value": True})]
            options.get_image.return_value = (self.gimp.ExportReturn.EXPORT, img)
            self.mod.export_dds(procedure, self.gimp.RunMode.NONINTERACTIVE, image, file, options, None, None, None)
            self.assertEqual(self.returned_error(procedure), self.message)
            img.delete.assert_called_once()

            procedure, file = mock.MagicMock(), mock.MagicMock()
            file.peek_path.return_value = os.path.join(HERE, "in.dds")
            self.mod._load(procedure, self.gimp.RunMode.NONINTERACTIVE, file)
            self.assertEqual(self.returned_error(procedure), self.message)
        self.assertFalse(os.path.exists(os.path.join(HERE, "out.dds")))

    def test_fallback_registration_matches_psdds(self):
        P.Psdds()  # the real DLL loads, so the values below are psdds's, not the fallback's
        d, top_q, presets = self.mod.registration_values()
        self.assertEqual((d.key(), top_q, presets),
                         (self.mod.FALLBACK.key(), self.mod.FALLBACK_TOP_Q, self.mod.FALLBACK_PRESETS))


if __name__ == "__main__":
    unittest.main()
