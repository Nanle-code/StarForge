;; Benign capability fixture: pure computation, no host imports.
;; Must instantiate and run successfully under the default WASM sandbox.
(module
  (func (export "run") (result i32)
    i32.const 42))
