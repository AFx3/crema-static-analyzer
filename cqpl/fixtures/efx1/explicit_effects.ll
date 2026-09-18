target triple = "x86_64-pc-linux-gnu"

declare void @read_arg(ptr nocapture nofree readonly) nofree nosync memory(argmem: read)
declare ptr @identity(ptr returned)

define i32 @main() {
entry:
  %slot = alloca i8, align 1
  call void @read_arg(ptr %slot)
  %q = call ptr @identity(ptr %slot)
  %same = icmp eq ptr %q, %slot
  %r = zext i1 %same to i32
  ret i32 %r
}
