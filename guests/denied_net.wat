;; Asks for network access. The static check rejects it before it runs.
(module
  (import "terrarium" "log" (func $log (param i32 i32)))
  (import "terrarium" "net_connect" (func $net (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "example.com:443")
  (func (export "run") (result i64)
    (call $log (i32.const 0) (i32.const 15))
    (i64.extend_i32_s (call $net (i32.const 0) (i32.const 15)))))
