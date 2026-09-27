#include <stddef.h>
#include <stdlib.h>
void *gate_seed_calloc(size_t n, size_t s) { return calloc(n,s); }
