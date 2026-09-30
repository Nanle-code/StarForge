;; Malicious capability fixture: tries to send data over a host socket via WASI.
;; The sandbox grants no imports, so instantiation must fail.
(module
  (import "wasi_snapshot_preview1" "sock_send"
    (func $sock_send (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "run") (result i32)
    (call $sock_send (i32.const 3) (i32.const 0) (i32.const 1) (i32.const 0) (i32.const 32))))
