import tempfile
import unittest
from pathlib import Path

from scripts.release_artifacts import EXPECTED_ARCHIVES, prepare_release


class ReleaseArtifactsTest(unittest.TestCase):
    def create_archives(self, directory: Path, names=EXPECTED_ARCHIVES) -> None:
        for name in names:
            path = directory / name
            path.write_bytes(name.encode("ascii"))

    def test_prepare_release_copies_all_archives_and_writes_checksums(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            source = root / "artifacts"
            destination = root / "release"
            source.mkdir()
            self.create_archives(source)

            published = prepare_release(source, destination)

            self.assertEqual([path.name for path in published], sorted(EXPECTED_ARCHIVES))
            manifest = (destination / "SHA256SUMS.txt").read_text(encoding="ascii")
            self.assertEqual(len(manifest.splitlines()), len(EXPECTED_ARCHIVES))
            for archive in EXPECTED_ARCHIVES:
                self.assertIn(f"  {archive}\n", manifest)

    def test_single_archive_is_a_missing_boundary(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            source = Path(temporary_directory)
            self.create_archives(source, {"starforge-linux-x86_64.tar.gz"})

            with self.assertRaisesRegex(ValueError, "missing release archives"):
                prepare_release(source, source / "release")

    def test_unexpected_archive_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            source = Path(temporary_directory)
            self.create_archives(source)
            (source / "starforge-linux-x86_64.deb").write_bytes(b"unsupported")

            with self.assertRaisesRegex(ValueError, "unsupported release archives"):
                prepare_release(source, source / "release")


if __name__ == "__main__":
    unittest.main()