// Synthetic conformance library. Compiled separately; never SVF analysis input.
// Only the second formal copy may be stored. This is valid when formals alias.
static void *saved;
void d3_observe(void *first, void *second) { (void)first; saved = second; }
void d3_unknown(void *p) { (void)p; }
