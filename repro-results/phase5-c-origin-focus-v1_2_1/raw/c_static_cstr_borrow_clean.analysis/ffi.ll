; ModuleID = '/home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_to_rust_alloc/c_static_cstr_borrow_clean/src/ffi.c'
source_filename = "/home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_to_rust_alloc/c_static_cstr_borrow_clean/src/ffi.c"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

@.str = private unnamed_addr constant [16 x i8] c"static-c-string\00", align 1

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i8* @c_static_string() #0 {
entry:
  ret i8* getelementptr inbounds ([16 x i8], [16 x i8]* @.str, i64 0, i64 0)
}

attributes #0 = { noinline nounwind optnone uwtable "frame-pointer"="all" "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 7, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 1}
!4 = !{i32 7, !"frame-pointer", i32 2}
!5 = !{!"Ubuntu clang version 14.0.0-1ubuntu1.1"}
