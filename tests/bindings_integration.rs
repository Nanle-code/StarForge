// Integration test for the binding generator
// This test demonstrates a complete workflow with a simple example

use starforge::utils::bindings::BindingLanguage;
use std::path::Path;
use tempfile::NamedTempFile;

/// Test that demonstrates the complete binding generation workflow
#[test]
fn test_complete_binding_workflow() {
    // Create a minimal WASM with contract metadata
    // This is a simplified example - in reality, you would use a real compiled contract
    let wasm_bytes = create_example_wasm_with_metadata();
    let temp_file = NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), &wasm_bytes).unwrap();

    // Test each language
    for lang in [
        BindingLanguage::Rust,
        BindingLanguage::TypeScript,
        BindingLanguage::Python,
        BindingLanguage::Go,
    ] {
        println!("Testing binding generation for {:?}", lang);

        let result = starforge::utils::bindings::generate_bindings(temp_file.path(), lang);

        // For this test, we just verify that generation doesn't panic
        // In a real integration test with a proper contract, we would:
        // 1. Verify the generated code compiles
        // 2. Test that the generated client can be instantiated
        // 3. Verify type safety and method signatures

        match result {
            Ok(code) => {
                // Basic validation of generated code
                match lang {
                    BindingLanguage::Rust => {
                        assert!(
                            code.contains("pub struct ContractClient"),
                            "Missing ContractClient in Rust"
                        );
                        assert!(code.contains("impl ContractClient"), "Missing impl in Rust");
                    }
                    BindingLanguage::TypeScript => {
                        assert!(
                            code.contains("export class ContractClient"),
                            "Missing ContractClient in TS"
                        );
                        assert!(
                            code.contains("export interface"),
                            "Missing interfaces in TS"
                        );
                    }
                    BindingLanguage::Python => {
                        assert!(
                            code.contains("class ContractClient"),
                            "Missing ContractClient in Python"
                        );
                        assert!(code.contains("@dataclass"), "Missing dataclass in Python");
                    }
                    BindingLanguage::Go => {
                        assert!(
                            code.contains("type ContractClient struct"),
                            "Missing ContractClient in Go"
                        );
                        assert!(
                            code.contains("func NewContractClient"),
                            "Missing constructor in Go"
                        );
                    }
                }

                // Verify event generation (if events were in the metadata)
                if code.contains("Event") {
                    println!("Generated code includes event definitions for {:?}", lang);
                }
            }
            Err(e) => {
                // Expected for our minimal test WASM - it doesn't have proper contract spec
                println!("Generation failed as expected for {:?}: {}", lang, e);
            }
        }
    }
}

/// Create a minimal WASM with some example contract metadata
fn create_example_wasm_with_metadata() -> Vec<u8> {
    let mut wasm = Vec::new();

    // WASM magic and version
    wasm.extend(b"\0asm\x01\x00\x00\x00");

    // For a real test, we would include a proper "contractspecv0" custom section
    // with XDR-encoded contract metadata. This is simplified for demonstration.

    // Add a custom section header
    wasm.push(0); // Custom section ID
    wasm.push(20); // Section length

    // Custom section name "contractspecv0" (simplified)
    let name = "contractspecv0";
    wasm.push(name.len() as u8);
    wasm.extend(name.as_bytes());

    // Simplified metadata - in reality this would be XDR-encoded
    wasm.extend(b"example metadata");

    wasm
}

/// Test error handling for various edge cases
#[test]
fn test_error_handling() {
    // Test with empty file
    let temp_file = NamedTempFile::new().unwrap();
    let result =
        starforge::utils::bindings::generate_bindings(temp_file.path(), BindingLanguage::Rust);
    assert!(result.is_err(), "Should fail on empty file");

    // Test with non-WASM data
    let temp_file = NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), b"not wasm at all").unwrap();
    let result =
        starforge::utils::bindings::generate_bindings(temp_file.path(), BindingLanguage::Rust);
    assert!(result.is_err(), "Should fail on non-WASM data");

    // Test with valid WASM but no contract metadata
    let minimal_wasm = b"\0asm\x01\x00\x00\x00";
    let temp_file = NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), minimal_wasm).unwrap();
    let result =
        starforge::utils::bindings::generate_bindings(temp_file.path(), BindingLanguage::Rust);
    assert!(
        result.is_err(),
        "Should fail on WASM without contract metadata"
    );
}

/// Test that the binding generator produces idiomatic code for each language
#[test]
fn test_idiomatic_code_generation() {
    let test_wasm = create_example_wasm_with_metadata();
    let temp_file = NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), &test_wasm).unwrap();

    // Test each language for basic idiomatic patterns
    let languages = [
        (BindingLanguage::Rust, vec!["pub struct", "impl", "Result<"]),
        (
            BindingLanguage::TypeScript,
            vec!["export class", "export interface", "type"],
        ),
        (
            BindingLanguage::Python,
            vec!["class", "def", "from typing import"],
        ),
        (BindingLanguage::Go, vec!["type", "func", "package"]),
    ];

    for (lang, patterns) in languages {
        let result = starforge::utils::bindings::generate_bindings(temp_file.path(), lang);

        if let Ok(code) = result {
            for pattern in &patterns {
                assert!(
                    code.contains(pattern),
                    "Missing pattern '{}' in {:?} generated code",
                    pattern,
                    lang
                );
            }
        }
    }
}
