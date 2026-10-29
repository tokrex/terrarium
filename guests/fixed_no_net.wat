;; The "repaired" version of denied_net.wat: data comes in, no network needed.
(module
  (import "terrarium" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "data passed in, no network needed")
  (func (export "run") (result i64)
    (call $log (i32.const 0) (i32.const 33))
    (i64.const 0)))
