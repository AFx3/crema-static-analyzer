target triple = "x86_64-pc-linux-gnu"

declare ptr @malloc(i64)
declare ptr @calloc(i64, i64)
declare ptr @realloc(ptr, i64)
declare void @free(ptr)

define i32 @main() {
entry:
  %p = call ptr @malloc(i64 16)
  %q = call ptr @calloc(i64 2, i64 8)
  %r = call ptr @realloc(ptr %p, i64 32)
  call void @free(ptr %r)
  call void @free(ptr %q)
  ret i32 0
}
