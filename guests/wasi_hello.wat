;; Imports WASI. Terrarium provides no WASI/WASIX, so this is "unknown import".
(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "run") (result i64)
    (i64.extend_i32_s
      (call $fd_write (i32.const 1) (i32.const 0) (i32.const 0) (i32.const 8)))))
