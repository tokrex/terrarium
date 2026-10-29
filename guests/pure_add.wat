;; No imports: needs no capabilities at all (runs even under tier-0).
(module
  (func (export "run") (result i64)
    (i64.add (i64.const 40) (i64.const 2))))
