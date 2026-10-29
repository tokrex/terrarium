;; Infinite loop: exhausts the fuel budget deterministically.
(module
  (func (export "run") (result i64)
    (loop $l (br $l))
    (i64.const 0)))
