; ModuleID = '/home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_to_rust_alloc/c_malloc_rust_string_from_raw_parts_ub/src/ffi.c'
source_filename = "/home/af/Documenti/a-phd/tests_and_target_repos/a-code_c_to_rust_alloc/c_malloc_rust_string_from_raw_parts_ub/src/ffi.c"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-pc-linux-gnu"

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i8* @c_alloc_hello() #0 {
entry:
  %p = alloca i8*, align 8
  %call = call noalias i8* @malloc(i64 noundef 6) #2
  store i8* %call, i8** %p, align 8
  %0 = load i8*, i8** %p, align 8
  %cmp = icmp ne i8* %0, null
  br i1 %cmp, label %if.then, label %if.end

if.then:                                          ; preds = %entry
  %1 = load i8*, i8** %p, align 8
  %arrayidx = getelementptr inbounds i8, i8* %1, i64 0
  store i8 104, i8* %arrayidx, align 1
  %2 = load i8*, i8** %p, align 8
  %arrayidx1 = getelementptr inbounds i8, i8* %2, i64 1
  store i8 101, i8* %arrayidx1, align 1
  %3 = load i8*, i8** %p, align 8
  %arrayidx2 = getelementptr inbounds i8, i8* %3, i64 2
  store i8 108, i8* %arrayidx2, align 1
  %4 = load i8*, i8** %p, align 8
  %arrayidx3 = getelementptr inbounds i8, i8* %4, i64 3
  store i8 108, i8* %arrayidx3, align 1
  %5 = load i8*, i8** %p, align 8
  %arrayidx4 = getelementptr inbounds i8, i8* %5, i64 4
  store i8 111, i8* %arrayidx4, align 1
  %6 = load i8*, i8** %p, align 8
  %arrayidx5 = getelementptr inbounds i8, i8* %6, i64 5
  store i8 0, i8* %arrayidx5, align 1
  br label %if.end

if.end:                                           ; preds = %if.then, %entry
  %7 = load i8*, i8** %p, align 8
  ret i8* %7
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
