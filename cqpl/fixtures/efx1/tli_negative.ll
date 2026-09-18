target triple = "x86_64-pc-linux-gnu"

; Wrong prototype: must not acquire the standard free contract.
declare void @free(i64)
; Explicit nobuiltin: EFX1 intentionally skips library inference.
declare ptr @malloc(i64) nobuiltin

define i32 @main() {
entry:
  %p = call ptr @malloc(i64 8)
  call void @free(i64 0)
  %isnull = icmp eq ptr %p, null
  %r = zext i1 %isnull to i32
  ret i32 %r
}
