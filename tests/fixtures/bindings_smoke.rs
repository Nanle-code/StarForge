use tempfile::NamedTempFile;
use starforge::utils::bindings::{self, BindingLanguage};

fn main() {
    println!("Testing enhanced binding generator...");

    let wasm = b"\0asm\x01\x00\x00\x00";
    let temp_file = NamedTempFile::new().unwrap();
    std::fs::write(temp_file.path(), wasm).unwrap();

    let languages = [
        BindingLanguage::Rust,
        BindingLanguage::TypeScript,
        BindingLanguage::Python,
        BindingLanguage::Go,
    ];

    for lang in languages {
        println!("\nTesting {:?}:", lang);
        match bindings::generate_bindings(temp_file.path(), lang) {
            Ok(code) => {
                println!("Generated code ({} bytes)", code.len());
                let expected = match lang {
                    BindingLanguage::Rust => "ContractClient",
                    BindingLanguage::TypeScript => "export class",
                    BindingLanguage::Python => "class ContractClient",
                    BindingLanguage::Go => "type ContractClient struct",
                };
                println!("Contains expected pattern: {}", code.contains(expected));
            }
            Err(error) => {
                println!("Binding generation rejected the minimal WASM: {}", error);
            }
        }
    }
}
