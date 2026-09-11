; ModuleID = '/home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_to_rust_alloc/c_malloc_rust_double_free_df/src/ffi.c'
source_filename = "/home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_to_rust_alloc/c_malloc_rust_double_free_df/src/ffi.c"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i32* @c_alloc_i32(i32 noundef %value) #0 {
entry:
  %value.addr = alloca i32, align 4
  %p = alloca i32*, align 8
  store i32 %value, i32* %value.addr, align 4
  %call = call noalias i8* @malloc(i64 noundef 4) #2
  %0 = bitcast i8* %call to i32*
  store i32* %0, i32** %p, align 8
  %1 = load i32*, i32** %p, align 8
  %cmp = icmp ne i32* %1, null
  br i1 %cmp, label %if.then, label %if.end

if.then:                                          ; preds = %entry
  %2 = load i32, i32* %value.addr, align 4
  %3 = load i32*, i32** %p, align 8
  store i32 %2, i32* %3, align 4
  br label %if.end

if.end:                                           ; preds = %if.then, %entry
  %4 = load i32*, i32** %p, align 8
  ret i32* %4
}

; Function Attrs: nounwind
declare noalias i8* @malloc(i64 noundef) #1

attributes #0 = { noinline nounwind optnone uwtable "frame-pointer"="all" "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #1 = { nounwind "frame-pointer"="all" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #2 = { nounwind }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 7, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 1}
!4 = !{i32 7, !"frame-pointer", i32 2}
!5 = !{!"Ubuntu clang version 14.0.0-1ubuntu1.1"}
