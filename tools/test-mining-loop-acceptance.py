"""Bounded readiness parsing regressions; no server or network required."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock

spec = importlib.util.spec_from_file_location(
    'mining_acceptance', Path(__file__).with_name('mining-loop-acceptance.py'))
acceptance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(acceptance)


class ReadinessTests(unittest.TestCase):
    def parse(self, output, offset=0):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'server.log'
            path.write_bytes(output)
            return acceptance.wait_for_server(Mock(poll=lambda: None), path, offset, .1)

    def test_ignores_previous_process_output(self):
        old = b'Arkline server listening on 127.0.0.1:1234\n'
        new = b'Arkline server listening on 127.0.0.1:54321\n'
        self.assertEqual(self.parse(old + new, len(old)), 54321)

    def test_rejects_invalid_ports_and_incomplete_output(self):
        for port in (b'0', b'65536', b'12junk'):
            with self.subTest(port=port), self.assertRaises(AssertionError):
                self.parse(b'Arkline server listening on 127.0.0.1:' + port + b'\n')
        with self.assertRaisesRegex(AssertionError, 'timeout'):
            self.parse(b'Arkline server listening on 127.0.0.1:1234')

    def test_child_exit_is_not_readiness(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'server.log'
            path.write_bytes(b'Arkline server listening on 127.0.0.1:1234\n')
            with self.assertRaisesRegex(AssertionError, 'exited'):
                acceptance.wait_for_server(Mock(poll=lambda: 1), path, 0)


if __name__ == '__main__':
    unittest.main()
