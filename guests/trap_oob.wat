;; Reads outside its linear memory: traps.
(module
  (memory 1)
  (func (export "run") (result i64)
    (i64.load (i32.const 70000))))
