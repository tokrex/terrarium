;; Uses the clock and randomness: the replay demo. The result mixes both, so
;; tampering with a recorded value changes the result and is detected.
(module
  (import "terrarium" "log" (func $log (param i32 i32)))
  (import "terrarium" "clock_now" (func $now (result i64)))
  (import "terrarium" "rand_u64" (func $rand (result i64)))
  (memory (export "memory") 1)
  (data (i32.const 0) "hello from the terrarium")
  (func (export "run") (result i64)
    (call $log (i32.const 0) (i32.const 24))
    (i64.xor (call $now) (call $rand))))
