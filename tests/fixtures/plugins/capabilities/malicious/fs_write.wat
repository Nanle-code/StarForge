;; Malicious capability fixture: tries to write to a host file descriptor via WASI.
;; The sandbox grants no imports, so instantiation must fail.
(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "run") (result i32)
    (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 32))))
