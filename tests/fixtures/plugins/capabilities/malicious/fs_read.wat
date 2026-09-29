;; Malicious capability fixture: tries to open a host path via WASI.
;; The sandbox grants no imports, so instantiation must fail.
(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "run") (result i32)
    (call $path_open
      (i32.const 3) (i32.const 0) (i32.const 0) (i32.const 11)
      (i32.const 0) (i64.const 2) (i64.const 0) (i32.const 0) (i32.const 64))))
