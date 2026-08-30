import tempfile
import unittest
from pathlib import Path

import validate_package_metadata as validator


class PackageMetadataTests(unittest.TestCase):
    def test_repository_and_docs_links_are_valid(self):
        validator.validate_manifests(Path(__file__).parents[1])

    def test_hyphenated_package_name_builds_docs_link(self):
        manifest = self._manifest(
            'name = "starforge-plugin-sdk"\n'
            'repository = "https://github.com/Josetic224/StarForge"\n'
            'homepage = "https://github.com/Josetic224/StarForge"\n'
            'documentation = "https://docs.rs/starforge-plugin-sdk"\n'
        )
        validator.validate_manifest(manifest)

    def test_placeholder_repository_is_rejected(self):
        manifest = self._manifest(
            'name = "starforge"\n'
            'repository = "https://github.com/YOUR_USERNAME/starforge"\n'
            'homepage = "https://github.com/Josetic224/StarForge"\n'
            'documentation = "https://docs.rs/starforge"\n'
        )
        with self.assertRaises(validator.MetadataValidationError):
            validator.validate_manifest(manifest)

    def test_malformed_manifest_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "Cargo.toml"
            manifest.write_text("[package\n", encoding="utf-8")
            with self.assertRaises(validator.MetadataValidationError):
                validator.validate_manifest(manifest)

    def test_unsupported_python_is_reported(self):
        original_tomllib = validator.tomllib
        validator.tomllib = None
        try:
            with self.assertRaisesRegex(RuntimeError, "Python 3.11"):
                validator.validate_manifest(Path("Cargo.toml"))
        finally:
            validator.tomllib = original_tomllib

    @staticmethod
    def _manifest(contents: str) -> Path:
        directory = tempfile.TemporaryDirectory()
        PackageMetadataTests._temporary_directories.append(directory)
        manifest = Path(directory.name) / "Cargo.toml"
        manifest.write_text("[package]\n" + contents, encoding="utf-8")
        return manifest


PackageMetadataTests._temporary_directories = []


if __name__ == "__main__":
    unittest.main()